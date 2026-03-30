use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use secrecy::ExposeSecret;
use tokio::join;
#[cfg(unix)]
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};

use polymarket_5m_bot::{config, data, pipeline};

use config::Config;
use data::market_discovery::{DiscoveryConfig, MarketDiscovery};
use data::polymarket::{AuthenticatedPolyClient, BalanceChecker, CtfRedeemer, PolymarketClient};
use pipeline::decider::{self, AccountState, DeciderConfig};
use pipeline::executor::{ExecuteContext, Executor};
use pipeline::price_source::PriceSource;
use pipeline::settler::{PendingPosition, Settler};
use pipeline::signal::Direction;

use crate::state::{BotState, MarketState};
use crate::tasks;

const PRICE_BUFFER_MAX: usize = 1000;

fn decimal(value: &'static str) -> Decimal {
    Decimal::from_str_exact(value).expect(value)
}

type TradeLogWriter = Arc<tokio::sync::Mutex<BufWriter<std::fs::File>>>;

pub(crate) struct Bot {
    config: Config,
    log_dir: String,
    price_source: Arc<PriceSource>,
    polymarket: Arc<PolymarketClient>,
    discovery: Arc<MarketDiscovery>,
    state: Arc<RwLock<BotState>>,
    account: Arc<RwLock<AccountState>>,
    settler: Arc<RwLock<Settler>>,
    executor: Executor,
    redeemer: Option<Arc<CtfRedeemer>>,
    balance_checker: Option<BalanceChecker>,
    market_state: Arc<RwLock<MarketState>>,
    shutdown: Arc<AtomicBool>,
    trade_log_writer: Option<TradeLogWriter>,
}

impl Bot {
    pub(crate) async fn new(config: Config, log_dir: String) -> Result<Self> {
        let price_source = Arc::new(PriceSource::new(
            config.price_source.source,
            &config.price_source.symbol,
            PRICE_BUFFER_MAX,
        ));
        let polymarket = Arc::new(PolymarketClient::new()?);

        let discovery_cfg = DiscoveryConfig {
            gamma_api_url: config.polyclob.gamma_api_url.clone(),
        };
        let discovery = Arc::new(MarketDiscovery::new(discovery_cfg));

        let auth_client = if config.trading.mode.is_live()
            && !config.trading.private_key.expose_secret().is_empty()
        {
            match AuthenticatedPolyClient::new(config.trading.private_key.expose_secret()).await {
                Ok(c) => {
                    tracing::info!("[INIT] Authenticated with Polymarket CLOB");
                    Some(c)
                }
                Err(e) => {
                    anyhow::bail!("[INIT] CLOB auth failed: {} — cannot run in live mode", e);
                }
            }
        } else {
            None
        };

        let executor = Executor::new(config.trading.mode, auth_client, config.execution.clone());

        let redeemer = if config.trading.mode.is_live()
            && !config.trading.private_key.expose_secret().is_empty()
        {
            let rpc = data::polymarket::rpc_url(config.trading.mode);
            tracing::info!("[INIT] CTF redeemer enabled for on-chain redemption");
            Some(Arc::new(CtfRedeemer::new(
                config.trading.private_key.expose_secret().to_owned(),
                rpc,
            )))
        } else {
            None
        };

        let initial_balance = if config.trading.mode.is_paper() {
            Self::load_balance(&log_dir)
                .await
                .unwrap_or_else(|| decimal("100"))
        } else {
            let rpc = data::polymarket::rpc_url(config.trading.mode);
            if let Some(ref r) = redeemer {
                let wallet = r
                    .wallet_address()
                    .map_err(|e| anyhow::anyhow!("[INIT] Wallet derivation failed: {}", e))?;
                let bc = data::polymarket::BalanceChecker::new(wallet, rpc.clone())
                    .await
                    .map_err(|e| anyhow::anyhow!("[INIT] BalanceChecker creation failed: {}", e))?;
                let on_chain = bc
                    .balance()
                    .await
                    .map_err(|e| anyhow::anyhow!("[INIT] Balance query failed: {}", e))?;
                // On-chain balance may be zero if funds are locked in positions.
                // Fall back to persisted balance file so we don't lose track of
                // working capital across restarts.
                if on_chain > Decimal::ZERO {
                    on_chain
                } else {
                    let saved = Self::load_balance(&log_dir).await;
                    if let Some(saved_bal) = saved {
                        tracing::info!(
                            "[INIT] On-chain balance is $0, restored ${:.2} from balance file",
                            saved_bal
                        );
                        saved_bal
                    } else {
                        on_chain
                    }
                }
            } else {
                anyhow::bail!("[INIT] Live mode requires redeemer for balance query")
            }
        };
        tracing::info!("[INIT] Starting balance: ${:.2}", initial_balance);
        Self::write_balance(&log_dir, initial_balance).await;

        let balance_checker = if config.trading.mode.is_live() {
            if let Some(ref r) = redeemer {
                match r.wallet_address() {
                    Ok(wallet) => {
                        let rpc = data::polymarket::rpc_url(config.trading.mode);
                        match BalanceChecker::new(wallet, rpc).await {
                            Ok(checker) => {
                                tracing::info!("[INIT] BalanceChecker connected");
                                Some(checker)
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "[INIT] BalanceChecker init failed: {}, will query per-tick",
                                    e
                                );
                                None
                            }
                        }
                    }
                    Err(_) => None,
                }
            } else {
                None
            }
        } else {
            None
        };

        let settler = Settler::new();
        let account = AccountState::new(initial_balance);

        // Initialize trade log writer for live mode
        let trade_log_writer = if config.trading.mode.is_live() {
            let trades_path = Path::new(&log_dir).join("trades.csv");
            match std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&trades_path)
            {
                Ok(file) => {
                    // Check if file is empty and write header
                    let metadata = file.metadata()?;
                    let mut writer = BufWriter::new(file);
                    if metadata.len() == 0 {
                        use std::io::Write;
                        writeln!(
                            writer,
                            "timestamp,direction,order_id,entry_price,cost,edge,balance,remaining_ms,yes_price,no_price,payoff_ratio"
                        )?;
                    }
                    Some(Arc::new(tokio::sync::Mutex::new(writer)))
                }
                Err(e) => {
                    tracing::warn!("[INIT] Failed to open trade log: {}", e);
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            config,
            log_dir,
            price_source,
            polymarket,
            discovery,
            state: Arc::new(RwLock::new(BotState::new())),
            account: Arc::new(RwLock::new(account)),
            settler: Arc::new(RwLock::new(settler)),
            executor,
            redeemer,
            balance_checker,
            market_state: Arc::new(RwLock::new(MarketState::default())),
            shutdown: Arc::new(AtomicBool::new(false)),
            trade_log_writer,
        })
    }

    async fn load_balance(log_dir: &str) -> Option<Decimal> {
        let content = tokio::fs::read_to_string(Path::new(log_dir).join("balance"))
            .await
            .ok()?;
        content.trim().parse().ok()
    }

    async fn write_balance(log_dir: &str, bal: Decimal) {
        let tmp = Path::new(log_dir).join("balance.tmp");
        let dst = Path::new(log_dir).join("balance");
        // Preserve full decimal precision to avoid accumulating rounding errors
        let text = format!("{}", bal.normalize());
        if let Err(e) = tokio::fs::write(&tmp, &text).await {
            tracing::warn!("[STATE] Failed to write balance: {}", e);
            return;
        }
        if let Err(e) = tokio::fs::rename(&tmp, &dst).await {
            tracing::warn!("[STATE] Failed to rename balance file: {}", e);
        }
    }

    pub(crate) async fn run(&mut self) -> Result<()> {
        tracing::info!(
            "[INIT] mode={} interval={}ms",
            self.config.trading.mode,
            self.config.polling.signal_interval_ms
        );

        self.refresh_market().await;
        let price_handles = self.price_source.clone().start(self.shutdown.clone()).await;

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
            tokio::signal::ctrl_c()
                .await
                .expect("failed to listen for ctrl-c");
            "SIGINT"
        };

        tokio::pin!(shutdown_signal);

        let mut settlement_handle = tasks::start_settlement_checker(
            self.settler.clone(),
            self.account.clone(),
            self.price_source.clone(),
            self.discovery.clone(),
            self.redeemer.clone(),
            self.log_dir.clone(),
            self.shutdown.clone(),
        );
        let mut refresher_handle = tasks::start_market_refresher(
            self.discovery.clone(),
            self.market_state.clone(),
            self.shutdown.clone(),
        );
        let mut status_handle = tasks::start_status_printer(
            self.price_source.clone(),
            self.account.clone(),
            self.settler.clone(),
            self.market_state.clone(),
            self.config.trading.mode,
            self.config.polling.status_interval_ms,
            self.shutdown.clone(),
        );
        let mut settlement_done = false;
        let mut refresher_done = false;
        let mut status_done = false;

        let mut tick = interval(Duration::from_millis(
            self.config.polling.signal_interval_ms,
        ));
        let mut flush_tick = interval(Duration::from_secs(30)); // Flush trade log every 30s

        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if let Err(e) = self.tick().await {
                        tracing::error!("[BOT] Tick error: {}", e);
                    }
                }
                _ = flush_tick.tick() => {
                    // Periodic flush of trade log buffer
                    if let Some(ref writer) = self.trade_log_writer {
                        let mut writer = writer.lock().await;
                        if let Err(e) = writer.flush() {
                            tracing::warn!("[LOG] Failed to flush trade log: {}", e);
                        }
                    }
                }
                signal = &mut shutdown_signal => {
                    tracing::info!("[BOT] Received {}, shutting down...", signal);
                    break;
                }
                result = &mut settlement_handle => {
                    settlement_done = true;
                    match result {
                        Ok(()) => tracing::error!("[BOT] Settlement checker exited unexpectedly"),
                        Err(e) => tracing::error!("[BOT] Settlement checker panicked: {}", e),
                    }
                    break;
                }
                result = &mut refresher_handle => {
                    refresher_done = true;
                    match result {
                        Ok(()) => tracing::error!("[BOT] Market refresher exited unexpectedly"),
                        Err(e) => tracing::error!("[BOT] Market refresher panicked: {}", e),
                    }
                    break;
                }
                result = &mut status_handle => {
                    status_done = true;
                    match result {
                        Ok(()) => tracing::error!("[BOT] Status printer exited unexpectedly"),
                        Err(e) => tracing::error!("[BOT] Status printer panicked: {}", e),
                    }
                    break;
                }
            }
        }

        self.shutdown.store(true, Ordering::Release);

        price_handles.ws_handle.abort();
        price_handles.receiver_handle.abort();

        let _ = tokio::time::timeout(Duration::from_secs(5), async {
            if !settlement_done {
                let _ = settlement_handle.await;
            }
            if !refresher_done {
                let _ = refresher_handle.await;
            }
            if !status_done {
                let _ = status_handle.await;
            }
        })
        .await;

        // Final flush of trade log on shutdown
        if let Some(ref writer) = self.trade_log_writer {
            let mut writer = writer.lock().await;
            if let Err(e) = writer.flush() {
                tracing::warn!("[LOG] Failed to flush trade log on shutdown: {}", e);
            }
        }

        Ok(())
    }

    fn decider_cfg(&self) -> DeciderConfig {
        DeciderConfig::from(&self.config)
    }

    async fn tick(&self) -> Result<()> {
        if let Some(ref checker) = self.balance_checker {
            match checker.balance().await {
                Ok(on_chain_bal) => {
                    let mut account = self.account.write().await;
                    account.balance = on_chain_bal;
                    drop(account); // Release lock early

                    // Debounced write - only write when balance changes significantly
                    let should_write = {
                        let state = self.state.read().await;
                        state.balance_state.should_write(on_chain_bal)
                    };

                    if should_write {
                        Self::write_balance(&self.log_dir, on_chain_bal).await;
                        let state = self.state.read().await;
                        state.balance_state.record_write(on_chain_bal);
                        tracing::debug!("[BALANCE] Wrote balance: ${:.2}", on_chain_bal);
                    }
                }
                Err(e) => {
                    tracing::warn!("[BAL] Failed to query on-chain USDC balance: {}", e);
                }
            }
        }

        let mkt = self.market_state.read().await.clone();

        let btc_price = match self.price_source.latest().await {
            Some(p) => p,
            None => return Ok(()),
        };

        let momentum_pct = self
            .price_source
            .momentum_pct(self.config.strategy.momentum_window_secs as i64)
            .await;

        let buf_len = self.price_source.buffer_len().await;
        if buf_len < 60 {
            let detail = format!("buffer={}/60", buf_len);
            self.state
                .write()
                .await
                .log_idle_change("buffer_filling", &detail);
            return Ok(());
        }

        let stale_threshold_ms = self.config.market.stale_threshold_ms;
        if let Some(last_ts) = self.price_source.last_tick_ms().await {
            let age = Utc::now().timestamp_millis() - last_ts;
            if age > stale_threshold_ms {
                tracing::warn!("[PRICE] BTC data stale ({}s), skipping trade", age / 1000);
                return Ok(());
            }
        }

        if !mkt.is_ready() {
            self.state
                .write()
                .await
                .log_idle_change("market_not_ready", "waiting for token IDs");
            return Ok(());
        }

        let min_ttl_ms = self.config.market.min_ttl_ms;
        if mkt.settlement_ms > 0 {
            let remaining = mkt.settlement_ms - Utc::now().timestamp_millis();
            if remaining < min_ttl_ms {
                let detail = format!("remaining={}s", remaining.max(0) / 1000);
                self.state
                    .write()
                    .await
                    .log_idle_change("ttl_too_short", &detail);
                return Ok(());
            }
        }

        // Fetch both prices in parallel
        let (yes_result, no_result) = join!(
            self.polymarket.fetch_mid_price(&mkt.token_yes),
            self.polymarket.fetch_mid_price(&mkt.token_no),
        );

        let (poly_yes_dec, poly_no_dec) = match (yes_result, no_result) {
            (Ok(y), Ok(n)) => {
                tracing::debug!("[PRICE] Yes={:.3} No={:.3} | buffer={}", y, n, buf_len);
                (Some(y), Some(n))
            }
            (Ok(y), Err(e)) => {
                tracing::warn!("[PRICE] Polymarket NO fetch failed: {}", e);
                (Some(y), None)
            }
            (Err(e), Ok(n)) => {
                tracing::warn!("[PRICE] Polymarket YES fetch failed: {}", e);
                (None, Some(n))
            }
            (Err(e), _) => {
                tracing::warn!("[PRICE] Polymarket fetch failed: {}", e);
                (None, None)
            }
        };
        let settlement_ms = mkt.settlement_ms;

        let today = Utc::now().format("%Y-%m-%d").to_string();
        {
            let mut acc = self.account.write().await;
            acc.reset_daily_if_needed(&today);
        }
        let account_read = self.account.read().await.clone();
        let decider_cfg = self.decider_cfg();
        let remaining_ms = (settlement_ms - Utc::now().timestamp_millis()).max(0);

        let decide_ctx = decider::DecideContext {
            market_yes: poly_yes_dec,
            market_no: poly_no_dec,
            remaining_ms,
            btc_momentum_pct: momentum_pct,
        };

        let decision = decider::decide(&decide_ctx, &account_read, &decider_cfg);

        match &decision {
            decider::Decision::Pass(reason) => {
                let mut st = self.state.write().await;
                let category =
                    reason.trim_end_matches(|c: char| c.is_ascii_digit() || c == '%' || c == '_');
                let prev_cat = st
                    .last_no_trade_reason
                    .trim_end_matches(|c: char| c.is_ascii_digit() || c == '%' || c == '_');
                let changed = category != prev_cat;
                if changed {
                    st.last_no_trade_reason = reason.clone();
                }
                if changed {
                    tracing::info!(
                        "[SKIP] {} | BTC=${:.0}",
                        reason,
                        btc_price.to_f64().unwrap_or(0.0)
                    );
                }
            }
            decider::Decision::Trade {
                direction,
                size_usdc: _,
                edge,
                payoff_ratio,
            } => {
                if self.settler.read().await.pending_count() > 0 {
                    return Ok(());
                }

                let fak_backoff_ms = self.config.risk.fak_backoff_ms as i64;
                {
                    let st = self.state.read().await;
                    if st.fak_market_ms == settlement_ms
                        && st.fak_rejections >= self.config.risk.max_fak_retries
                    {
                        return Ok(());
                    }
                    if st.last_fak_rejection_ms > 0 {
                        let elapsed =
                            chrono::Utc::now().timestamp_millis() - st.last_fak_rejection_ms;
                        if elapsed < fak_backoff_ms {
                            tracing::debug!(
                                "[FAK] backoff {}ms remaining",
                                fak_backoff_ms - elapsed
                            );
                            return Ok(());
                        }
                    }
                }

                let cheap_price = match direction {
                    Direction::Up => poly_yes_dec,
                    Direction::Down => poly_no_dec,
                };
                let cheap_price = match cheap_price {
                    Some(p) => p,
                    None => {
                        tracing::warn!(
                            "[TRADE] {} price missing for {}, skipping trade",
                            if matches!(direction, Direction::Up) {
                                "YES"
                            } else {
                                "NO"
                            },
                            direction.as_str()
                        );
                        return Ok(());
                    }
                };

                tracing::info!(
                    "[TRADE] {} @ {:.3} edge={:.0}% payoff={:.1}x BTC=${:.0}",
                    direction.as_str(),
                    cheap_price,
                    (*edge * decimal("100")).round_dp(0),
                    payoff_ratio,
                    btc_price.to_f64().unwrap_or(0.0),
                );

                let order = self
                    .executor
                    .execute(&ExecuteContext {
                        decision: &decision,
                        token_yes: &mkt.token_yes,
                        token_no: &mkt.token_no,
                        poly_yes: poly_yes_dec,
                        poly_no: poly_no_dec,
                        settlement_time_ms: settlement_ms,
                        btc_price,
                    })
                    .await;

                if order.is_none() && self.config.trading.mode.is_live() {
                    let mut st = self.state.write().await;
                    st.last_fak_rejection_ms = chrono::Utc::now().timestamp_millis();
                    if st.fak_market_ms != settlement_ms {
                        st.fak_rejections = 0;
                        st.fak_market_ms = settlement_ms;
                    }
                    st.fak_rejections += 1;
                    let max = self.config.risk.max_fak_retries;
                    if st.fak_rejections >= max {
                        tracing::warn!(
                            "[EXEC] {} FAK rejections, giving up on this market window",
                            st.fak_rejections
                        );
                    }
                }

                if let Some(order) = order {
                    {
                        let mut acc = self.account.write().await;
                        let today = Utc::now().format("%Y-%m-%d").to_string();
                        acc.reset_daily_if_needed(&today);
                        acc.record_trade(order.cost);
                    }

                    self.settler.write().await.add_position(PendingPosition {
                        direction: order.direction,
                        size_usdc: order.size_usdc,
                        entry_price: order.entry_price,
                        filled_shares: order.filled_shares,
                        cost: order.cost,
                        settlement_time_ms: order.settlement_time_ms,
                        entry_btc_price: order.entry_btc_price,
                        condition_id: Arc::clone(&mkt.condition_id),
                        market_slug: Arc::clone(&mkt.market_slug),
                    });

                    let bal = self.account.read().await.balance;
                    Self::write_balance(&self.log_dir, bal).await;

                    // Write to buffered trade log
                    if let Some(ref writer) = self.trade_log_writer {
                        let order_id_short: String = order.order_id.chars().take(8).collect();
                        let log_line = format!(
                            "{},{},{},{:.3},{:.2},{:.1},{:.2},{}s,{:.3},{:.3},{:.1}x\n",
                            Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
                            order.direction.as_str(),
                            order_id_short,
                            order.entry_price,
                            order.cost,
                            (*edge * decimal("100")).round_dp(1),
                            bal,
                            remaining_ms / 1000,
                            poly_yes_dec.unwrap_or_default(),
                            poly_no_dec.unwrap_or_default(),
                            payoff_ratio,
                        );

                        let mut writer = writer.lock().await;
                        use std::io::Write;
                        if let Err(e) = writer.write_all(log_line.as_bytes()) {
                            tracing::warn!("[LOG] trades.csv write failed: {}", e);
                        }
                        // BufWriter will flush when buffer is full or on drop
                    }
                }
            }
        }

        Ok(())
    }

    async fn refresh_market(&self) {
        match self.discovery.discover().await {
            Ok(active) => {
                tracing::info!(
                    "[MKT] {} ends {} cid={}",
                    active.market.slug,
                    active.end_date,
                    &active.condition_id.get(..8).unwrap_or(&active.condition_id)
                );
                *self.market_state.write().await = MarketState {
                    token_yes: active.token_id_yes.into(),
                    token_no: active.token_id_no.into(),
                    condition_id: active.condition_id.into(),
                    market_slug: active.market.slug.into(),
                    settlement_ms: active.end_date.timestamp_millis(),
                };
            }
            Err(e) => {
                tracing::warn!("[MKT] discovery failed: {}", e);
            }
        }
    }
}
