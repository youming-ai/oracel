use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use rust_decimal::Decimal;
use tokio::join;
#[cfg(unix)]
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};

use binance_5m_bot::config::Config;
use binance_5m_bot::data::binance_prediction::BinancePredictionClient;
use binance_5m_bot::pipeline::decider::{self, AccountState, DeciderConfig, Decision};
use binance_5m_bot::pipeline::executor::{ExecuteContext, ExecutionOutcome, Executor};
use binance_5m_bot::pipeline::price_source::PriceSource;
use binance_5m_bot::pipeline::settler::{PendingPosition, Settler};
use binance_5m_bot::trade_log::TradeLog;
use binance_5m_bot::tui::state::{TradeRow, TuiState};
use binance_5m_bot::util;

use crate::state::{BotState, MarketState};
use crate::tasks;

async fn wait_for_shutdown(shutdown: Arc<AtomicBool>) {
    while !shutdown.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

pub(crate) struct Bot {
    config: Config,
    log_dir: String,
    price_source: Arc<PriceSource>,
    prediction: Arc<BinancePredictionClient>,
    state: Arc<RwLock<BotState>>,
    account: Arc<RwLock<AccountState>>,
    settler: Arc<RwLock<Settler>>,
    executor: Executor,
    market_state: Arc<RwLock<MarketState>>,
    shutdown: Arc<AtomicBool>,
    trade_log: TradeLog,
    tui_state: Arc<RwLock<TuiState>>,
    last_live_balance_refresh_ms: i64,
}

impl Bot {
    pub(crate) async fn new(
        config: Config,
        log_dir: String,
        tui_state: Arc<RwLock<TuiState>>,
    ) -> Result<Self> {
        let price_source = Arc::new(PriceSource::new(
            &config.price_source.symbol,
            config.price_source.buffer_max,
        ));
        let prediction = Arc::new(
            BinancePredictionClient::connect(
                &config.binance_prediction,
                config.trading.mode.is_live(),
            )
            .await?,
        );
        let initial_balance = if config.trading.mode.is_paper() {
            Self::load_balance(&log_dir)
                .await
                .unwrap_or(config.trading.paper_starting_balance)
        } else {
            prediction.payment_balance().await?
        };
        tracing::info!(
            "[INIT] Binance Prediction {} balance: {:.2} USDT",
            config.trading.mode,
            initial_balance
        );
        util::write_balance(&log_dir, initial_balance).await;

        let settler = Settler::load(&log_dir).await?;
        let restored_awaiting_execution = !settler.awaiting_reconciliation().is_empty();
        if settler.pending_count() > 0 {
            tracing::warn!(
                "[INIT] restored {} Binance Prediction pending order(s)/position(s)",
                settler.pending_count()
            );
        }
        let trade_log = TradeLog::open(&log_dir)
            .map_err(|error| anyhow::anyhow!("failed to open Binance trade log: {error}"))?;
        Ok(Self {
            executor: Executor::new(config.trading.mode, Arc::clone(&prediction)),
            config,
            log_dir,
            price_source,
            prediction,
            state: Arc::new(RwLock::new(BotState {
                execution_halted: restored_awaiting_execution,
                ..BotState::new()
            })),
            account: Arc::new(RwLock::new(AccountState::new(initial_balance))),
            settler: Arc::new(RwLock::new(settler)),
            market_state: Arc::new(RwLock::new(MarketState::default())),
            shutdown: Arc::new(AtomicBool::new(false)),
            trade_log,
            tui_state,
            last_live_balance_refresh_ms: 0,
        })
    }

    pub(crate) fn shutdown_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.shutdown)
    }

    async fn load_balance(log_dir: &str) -> Option<Decimal> {
        tokio::fs::read_to_string(Path::new(log_dir).join("balance"))
            .await
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    pub(crate) async fn run(&mut self) -> Result<()> {
        tracing::info!(
            "[INIT] venue=binance_prediction mode={} symbol={} interval={}ms",
            self.config.trading.mode,
            self.config.price_source.symbol,
            self.config.polling.signal_interval_ms,
        );
        self.refresh_market().await;
        let price_handles = self
            .price_source
            .clone()
            .start(Arc::clone(&self.shutdown))
            .await;

        #[cfg(unix)]
        let mut sigint = signal(SignalKind::interrupt())?;
        #[cfg(unix)]
        let mut sigterm = signal(SignalKind::terminate())?;
        #[cfg(unix)]
        let shutdown_signal = async {
            tokio::select! {
                _ = sigint.recv() => "SIGINT",
                _ = sigterm.recv() => "SIGTERM",
            }
        };
        #[cfg(not(unix))]
        let shutdown_signal = async {
            tokio::signal::ctrl_c().await?;
            "SIGINT"
        };
        tokio::pin!(shutdown_signal);

        let mut settlement_handle = tasks::start_settlement_checker(
            self.config.trading.mode,
            Arc::clone(&self.prediction),
            Arc::clone(&self.settler),
            Arc::clone(&self.account),
            Arc::clone(&self.price_source),
            self.log_dir.clone(),
            self.trade_log.clone_handle(),
            Arc::clone(&self.shutdown),
            self.config.polling.settlement_check_secs,
            Arc::clone(&self.tui_state),
        );
        let mut refresher_handle = tasks::start_market_refresher(
            Arc::clone(&self.prediction),
            Arc::clone(&self.market_state),
            Arc::clone(&self.shutdown),
            self.config.polling.market_refresh_secs,
        );
        let mut status_handle = tasks::start_status_printer(
            Arc::clone(&self.price_source),
            Arc::clone(&self.account),
            Arc::clone(&self.settler),
            Arc::clone(&self.market_state),
            self.config.trading.mode,
            self.config.polling.status_interval_ms,
            Arc::clone(&self.shutdown),
        );
        let mut settlement_done = false;
        let mut refresher_done = false;
        let mut status_done = false;
        let mut tick = interval(Duration::from_millis(
            self.config.polling.signal_interval_ms,
        ));
        let mut flush = interval(Duration::from_secs(self.config.misc.trade_log_flush_secs));

        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if let Err(error) = self.tick().await {
                        tracing::error!("[BOT] tick error: {error:#}");
                    }
                }
                _ = flush.tick() => self.trade_log.flush().await,
                signal = &mut shutdown_signal => {
                    tracing::info!("[BOT] received {signal}, shutting down");
                    break;
                }
                _ = wait_for_shutdown(Arc::clone(&self.shutdown)) => {
                    tracing::info!("[BOT] shutdown requested by TUI");
                    break;
                }
                result = &mut settlement_handle => {
                    settlement_done = true;
                    tracing::error!("[BOT] settlement task stopped: {result:?}");
                    break;
                }
                result = &mut refresher_handle => {
                    refresher_done = true;
                    tracing::error!("[BOT] market refresher stopped: {result:?}");
                    break;
                }
                result = &mut status_handle => {
                    status_done = true;
                    tracing::error!("[BOT] status task stopped: {result:?}");
                    break;
                }
            }
        }

        self.shutdown.store(true, Ordering::Release);
        price_handles.ws_handle.abort();
        price_handles.receiver_handle.abort();
        let _ = tokio::time::timeout(
            Duration::from_secs(self.config.misc.shutdown_timeout_secs),
            async {
                if !settlement_done {
                    let _ = settlement_handle.await;
                }
                if !refresher_done {
                    let _ = refresher_handle.await;
                }
                if !status_done {
                    let _ = status_handle.await;
                }
            },
        )
        .await;
        if let Err(error) = self.settler.read().await.persist(&self.log_dir).await {
            tracing::error!("[STATE] failed to persist Binance positions on shutdown: {error:#}");
        }
        self.trade_log.flush().await;
        Ok(())
    }

    fn decider_config(&self) -> DeciderConfig {
        DeciderConfig::from(&self.config)
    }

    async fn refresh_live_balance_if_due(&mut self, now_ms: i64) {
        if self.config.trading.mode.is_paper()
            || now_ms.saturating_sub(self.last_live_balance_refresh_ms)
                < self.config.polling.status_interval_ms as i64
        {
            return;
        }
        self.last_live_balance_refresh_ms = now_ms;
        match self.prediction.payment_balance().await {
            Ok(balance) => {
                self.account.write().await.balance = balance;
                util::write_balance(&self.log_dir, balance).await;
            }
            Err(error) => {
                tracing::warn!("[BAL] Binance Prediction balance refresh failed: {error:#}")
            }
        }
    }

    async fn tick(&mut self) -> Result<()> {
        let now_ms = Utc::now().timestamp_millis();
        self.refresh_live_balance_if_due(now_ms).await;

        {
            let account = self.account.read().await;
            let pending = self.settler.read().await.pending_count();
            let mut tui = self.tui_state.write().await;
            tui.update_from_account(
                account.balance,
                account.pnl(),
                account.total_wins,
                account.total_losses,
                account.consecutive_wins,
                account.consecutive_losses,
            );
            tui.set_pending_count(pending);
        }

        let btc_price = match self.price_source.latest().await {
            Some(price) => price,
            None => return Ok(()),
        };
        let market = self.market_state.read().await.active.clone();
        {
            let mut tui = self.tui_state.write().await;
            tui.set_btc_price(btc_price);
            if let Some(market) = &market {
                tui.update_market(&market.slug, market.end_ms);
            }
        }

        let buffer_len = self.price_source.buffer_len().await;
        if buffer_len < self.config.price_source.buffer_min_ticks {
            self.idle(
                "buffer_filling",
                &format!(
                    "buffer={buffer_len}/{}",
                    self.config.price_source.buffer_min_ticks
                ),
            )
            .await;
            return Ok(());
        }
        if self
            .price_source
            .last_tick_ms()
            .await
            .is_some_and(|tick_ms| {
                now_ms.saturating_sub(tick_ms) > self.config.price_source.stale_threshold_ms
            })
        {
            self.idle(
                "stale_binance_price",
                "waiting for BTCUSDT WebSocket update",
            )
            .await;
            return Ok(());
        }
        let Some(market) = market else {
            self.idle(
                "market_not_ready",
                "waiting for Binance Prediction discovery",
            )
            .await;
            return Ok(());
        };

        {
            let awaiting = self.settler.read().await.awaiting_reconciliation();
            let mut state = self.state.write().await;
            if state.execution_halted && awaiting.is_empty() {
                state.execution_halted = false;
                tracing::info!("[EXEC] Binance order reconciliation completed; resuming entries");
            }
            if state.execution_halted {
                self.tui_state
                    .write()
                    .await
                    .set_decision("HALTED: awaiting Binance order reconciliation".into());
                return Ok(());
            }
        }

        let order_size = self.config.strategy.position_size_usdt;
        let (up_result, down_result) = join!(
            self.prediction
                .fetch_buy_quote(&market, decider::Direction::Up, order_size),
            self.prediction
                .fetch_buy_quote(&market, decider::Direction::Down, order_size),
        );
        let up_quote = up_result
            .inspect_err(|error| tracing::debug!("[BOOK] UP quote unavailable: {error:#}"))
            .ok();
        let down_quote = down_result
            .inspect_err(|error| tracing::debug!("[BOOK] DOWN quote unavailable: {error:#}"))
            .ok();
        let quote_is_fresh = |timestamp_ms: i64| {
            now_ms.saturating_sub(timestamp_ms)
                <= self.config.strategy.order_book_stale_threshold_ms
        };
        let remaining_ms = (market.end_ms - now_ms).max(0);
        let context = decider::DecideContext {
            now_ms,
            market_fee_rate_bps: Some(market.fee_rate_bps),
            up_quote: up_quote.filter(|quote| quote_is_fresh(quote.timestamp_ms)),
            down_quote: down_quote.filter(|quote| quote_is_fresh(quote.timestamp_ms)),
            remaining_ms,
            reference_price: Some(market.reference_price),
            current_price: Some(btc_price),
            sigma_per_second: self
                .price_source
                .realized_sigma_per_second(
                    self.config.strategy.volatility_lookback_secs,
                    self.config.strategy.min_volatility_samples,
                )
                .await,
            binance_trend_15s_pct: self.price_source.trend_pct(15).await,
            binance_trend_30s_pct: self.price_source.trend_pct(30).await,
        };
        let today = Utc::now().format("%Y-%m-%d").to_string();
        self.account.write().await.reset_daily_if_needed(&today);
        let account = self.account.read().await.clone();
        let mut decision = decider::decide(&context, &account, &self.decider_config());
        if matches!(decision, Decision::Trade { .. }) {
            let tracker = self.settler.read().await;
            if tracker.has_market(market.market_topic_id) {
                decision = Decision::Pass("market_already_traded".into());
            } else if tracker.pending_count() >= self.config.strategy.max_unsettled_positions {
                decision = Decision::Pass("max_unsettled_positions".into());
            }
        }
        self.trade_log
            .log_observation(&market, btc_price, &context, &decision)
            .await;

        match &decision {
            Decision::Pass(reason) => self.record_pass(reason, btc_price).await,
            Decision::Trade {
                direction,
                edge,
                model_probability,
                payoff_ratio,
                entry_price,
                size_usdt,
                ..
            } => {
                self.tui_state.write().await.set_decision(format!(
                    "TRADE {} p={:.1}% edge={:.1}%",
                    direction.as_str(),
                    *model_probability * Decimal::from(100),
                    *edge * Decimal::from(100),
                ));
                tracing::info!(
                    "[TRADE] Binance {} @ {:.3} p={:.1}% edge={:.1}% BTC=${:.0}",
                    direction.as_str(),
                    *entry_price,
                    *model_probability * Decimal::from(100),
                    *edge * Decimal::from(100),
                    btc_price,
                );
                match self
                    .executor
                    .execute(&ExecuteContext {
                        decision: &decision,
                        market: &market,
                        btc_price,
                    })
                    .await
                {
                    Ok(ExecutionOutcome::Filled(order)) => {
                        let total_cost = order.total_cost();
                        let balance = {
                            let mut account = self.account.write().await;
                            account.record_trade(total_cost);
                            account.balance
                        };
                        let position = PendingPosition::from_filled(&market, order.clone());
                        self.settler.write().await.add(position);
                        if let Err(error) = self.settler.read().await.persist(&self.log_dir).await {
                            tracing::error!(
                                "[STATE] failed to persist Binance position: {error:#}"
                            );
                        }
                        if self.config.trading.mode.is_paper() {
                            util::write_balance(&self.log_dir, balance).await;
                        }
                        self.trade_log
                            .log_entry(
                                &market,
                                &order,
                                *edge,
                                balance,
                                remaining_ms,
                                context.up_quote,
                                context.down_quote,
                                *payoff_ratio,
                            )
                            .await;
                        self.tui_state.write().await.add_trade(TradeRow {
                            time: Utc::now(),
                            market_topic_id: market.market_topic_id.to_string(),
                            direction: order.direction.as_str().to_string(),
                            entry_price: order.entry_price,
                            cost: total_cost,
                            edge: *edge * Decimal::from(100),
                            result: "PENDING".into(),
                            pnl: None,
                        });
                    }
                    Ok(ExecutionOutcome::AwaitingReconciliation { order_id }) => {
                        self.settler
                            .write()
                            .await
                            .add(PendingPosition::awaiting_reconciliation(
                                &market,
                                *direction,
                                *size_usdt,
                                order_id.clone(),
                                btc_price,
                            ));
                        if let Err(error) = self.settler.read().await.persist(&self.log_dir).await {
                            tracing::error!(
                                "[STATE] failed to persist uncertain Binance order: {error:#}"
                            );
                        }
                        self.state.write().await.execution_halted = true;
                        self.tui_state
                            .write()
                            .await
                            .set_decision("HALTED: Binance order awaiting reconciliation".into());
                        tracing::error!(
                            "[EXEC] Binance order {} accepted but fill status is unknown; entries halted",
                            order_id
                        );
                    }
                    Ok(ExecutionOutcome::Unfilled) => {
                        self.tui_state
                            .write()
                            .await
                            .set_decision("PASS: Binance FOK unfilled".into());
                    }
                    Err(error) => {
                        if self.config.trading.mode.is_live() {
                            self.state.write().await.execution_halted = true;
                            self.tui_state
                                .write()
                                .await
                                .set_decision("HALTED: Binance execution error".into());
                            tracing::error!(
                                "[EXEC] Binance live execution error; entries halted for safety: {error:#}"
                            );
                        } else {
                            tracing::warn!("[EXEC] Binance paper execution error: {error:#}");
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn idle(&self, reason: &str, detail: &str) {
        self.tui_state
            .write()
            .await
            .set_decision(format!("IDLE: {reason}"));
        self.state.write().await.log_idle_change(reason, detail);
    }

    async fn record_pass(&self, reason: &str, btc_price: Decimal) {
        self.tui_state
            .write()
            .await
            .set_decision(format!("PASS: {reason}"));
        let mut state = self.state.write().await;
        let category = reason.trim_end_matches(|character: char| {
            character.is_ascii_digit() || character == '%' || character == '_' || character == '.'
        });
        let previous = state
            .last_no_trade_reason
            .trim_end_matches(|character: char| {
                character.is_ascii_digit()
                    || character == '%'
                    || character == '_'
                    || character == '.'
            });
        if category != previous {
            state.last_no_trade_reason = reason.to_string();
            tracing::debug!("[SKIP] {reason} | BTC=${btc_price:.0}");
        }
    }

    async fn refresh_market(&self) {
        match self
            .prediction
            .discover_active_market(Utc::now().timestamp_millis())
            .await
        {
            Ok(market) => {
                tracing::info!(
                    "[MKT] Binance Prediction {} topic={} ends={}",
                    market.slug,
                    market.market_topic_id,
                    market.end_ms,
                );
                self.market_state.write().await.active = Some(market);
            }
            Err(error) => tracing::warn!("[MKT] Binance Prediction discovery failed: {error:#}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;

    use super::Bot;

    #[tokio::test]
    async fn load_balance_parses_persisted_decimal() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("balance"), "100.50")
            .await
            .unwrap();
        assert_eq!(
            Bot::load_balance(dir.path().to_str().unwrap()).await,
            Some(Decimal::new(10050, 2))
        );
    }

    #[tokio::test]
    async fn load_balance_trims_surrounding_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("balance"), "  42 \n")
            .await
            .unwrap();
        assert_eq!(
            Bot::load_balance(dir.path().to_str().unwrap()).await,
            Some(Decimal::new(42, 0))
        );
    }

    #[tokio::test]
    async fn load_balance_returns_none_when_missing_empty_or_invalid() {
        // Missing file, empty file, and garbage all fall back to None so the caller can
        // reuse the configured paper starting balance instead of crashing on restart.
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Bot::load_balance(dir.path().to_str().unwrap()).await, None);

        tokio::fs::write(dir.path().join("balance"), "")
            .await
            .unwrap();
        assert_eq!(Bot::load_balance(dir.path().to_str().unwrap()).await, None);

        tokio::fs::write(dir.path().join("balance"), "not-a-number")
            .await
            .unwrap();
        assert_eq!(Bot::load_balance(dir.path().to_str().unwrap()).await, None);
    }
}
