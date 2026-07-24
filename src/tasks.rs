use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::RwLock;
use tokio::time::Duration;

use binance_5m_bot::config::TradingMode;
use binance_5m_bot::data::binance_prediction::{
    BinancePredictionClient, LiveSettlement, OrderReconciliation,
};
use binance_5m_bot::pipeline::decider::AccountState;
use binance_5m_bot::pipeline::executor::OrderResult;
use binance_5m_bot::pipeline::price_source::PriceSource;
use binance_5m_bot::pipeline::settler::{SettlementResult, Settler};
use binance_5m_bot::trade_log::TradeLogHandle;
use binance_5m_bot::tui::state::{TradeRow, TuiState};
use binance_5m_bot::util;

use crate::state::MarketState;

pub(crate) fn start_market_refresher(
    client: Arc<BinancePredictionClient>,
    market_state: Arc<RwLock<MarketState>>,
    shutdown: Arc<AtomicBool>,
    refresh_secs: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(refresh_secs));
        loop {
            interval.tick().await;
            if shutdown.load(Ordering::Acquire) {
                tracing::debug!("[TASK] market refresher shutting down");
                break;
            }
            match client
                .discover_active_market(Utc::now().timestamp_millis())
                .await
            {
                Ok(market) => {
                    let changed = market_state
                        .read()
                        .await
                        .active
                        .as_ref()
                        .is_none_or(|current| current.market_topic_id != market.market_topic_id);
                    if changed {
                        tracing::info!(
                            "[MKT] Binance Prediction {} topic={} ends={}",
                            market.slug,
                            market.market_topic_id,
                            market.end_ms,
                        );
                        market_state.write().await.active = Some(market);
                    }
                }
                Err(error) => {
                    tracing::warn!("[MKT] Binance Prediction discovery failed: {error:#}");
                }
            }
        }
    })
}

pub(crate) fn start_status_printer(
    price_source: Arc<PriceSource>,
    account: Arc<RwLock<AccountState>>,
    settler: Arc<RwLock<Settler>>,
    market_state: Arc<RwLock<MarketState>>,
    mode: TradingMode,
    status_interval_ms: u64,
    shutdown: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(status_interval_ms));
        loop {
            interval.tick().await;
            if shutdown.load(Ordering::Acquire) {
                tracing::debug!("[TASK] status printer shutting down");
                break;
            }
            let btc = price_source.latest().await.unwrap_or_default();
            let account = account.read().await;
            let market = market_state.read().await.active.clone();
            let pending = settler.read().await.pending_count();
            let ttl = market
                .as_ref()
                .map(|market| (market.end_ms - Utc::now().timestamp_millis()).max(0) / 1_000)
                .unwrap_or_default();
            tracing::debug!(
                "[STATUS] binance {} | BTC=${:.0} | bal={:.2} pnl={:+.2} | {}/{} streak={} | pending={} | ttl={}m{}s",
                mode,
                btc.round_dp(0),
                account.balance,
                account.pnl(),
                account.total_wins,
                account.total_losses,
                if account.consecutive_wins > 0 {
                    format!("+{}", account.consecutive_wins)
                } else if account.consecutive_losses > 0 {
                    format!("-{}", account.consecutive_losses)
                } else {
                    "0".to_string()
                },
                pending,
                ttl / 60,
                ttl % 60,
            );
        }
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn start_settlement_checker(
    mode: TradingMode,
    client: Arc<BinancePredictionClient>,
    settler: Arc<RwLock<Settler>>,
    account: Arc<RwLock<AccountState>>,
    price_source: Arc<PriceSource>,
    log_dir: String,
    trade_log: TradeLogHandle,
    shutdown: Arc<AtomicBool>,
    settlement_check_secs: u64,
    tui_state: Arc<RwLock<TuiState>>,
    persist_halt: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(settlement_check_secs));
        loop {
            interval.tick().await;
            if shutdown.load(Ordering::Acquire) {
                tracing::debug!("[TASK] settlement checker shutting down");
                break;
            }

            reconcile_pending_orders(
                mode,
                &client,
                &settler,
                &account,
                &trade_log,
                &log_dir,
                &tui_state,
                &persist_halt,
            )
            .await;

            let due = settler.read().await.due_positions();
            let mut settled = Vec::<(SettlementResult, bool)>::new();
            for position in due {
                let settlement = if mode.is_paper() {
                    match client
                        .paper_resolution(position.market_topic_id, Utc::now().timestamp_millis())
                        .await
                    {
                        Ok(Some(winner)) => settler
                            .write()
                            .await
                            .settle_paper(position.market_topic_id, winner)
                            .map(|result| (result, false)),
                        Ok(None) => None,
                        Err(error) => {
                            tracing::warn!(
                                "[SETTLE] Binance paper resolution failed for {}: {error:#}",
                                position.market_topic_id
                            );
                            None
                        }
                    }
                } else {
                    match client
                        .live_settlement(
                            position.market_topic_id,
                            &position.token_id,
                            position.settlement_time_ms,
                        )
                        .await
                    {
                        Ok(Some(LiveSettlement {
                            won,
                            payout,
                            pnl,
                            redeem_status,
                        })) => {
                            let redeem_needed = won && redemption_needed(redeem_status.as_deref());
                            settler
                                .write()
                                .await
                                .settle_live(position.market_topic_id, won, payout, pnl)
                                .map(|result| (result, redeem_needed))
                        }
                        Ok(None) => None,
                        Err(error) => {
                            tracing::warn!(
                                "[SETTLE] Binance live settlement failed for {}: {error:#}",
                                position.market_topic_id
                            );
                            None
                        }
                    }
                };
                if let Some(result) = settlement {
                    settled.push(result);
                }
            }

            if settled.is_empty() {
                continue;
            }
            if let Err(error) = settler.read().await.persist(&log_dir).await {
                tracing::error!("[STATE] failed to persist Binance pending positions: {error:#}");
                util::halt_on_state_write_failure(&persist_halt, &log_dir).await;
            }

            let now_ms = Utc::now().timestamp_millis();
            let current_price = price_source.latest().await.unwrap_or_default();
            let balance = {
                let mut account = account.write().await;
                account.reset_daily_if_needed(&Utc::now().format("%Y-%m-%d").to_string());
                for (result, _) in &settled {
                    account.record_settlement(
                        result.won,
                        result.payout,
                        result.pnl,
                        mode.is_paper(),
                        now_ms,
                    );
                }
                // Persisted under the write lock, after the settler snapshot above,
                // so a concurrent trade-tick persist cannot reorder ahead of it.
                // Bounded residual: a crash between the settler persist and this
                // write permanently forgets at most this batch's risk contribution
                // (the positions are already gone from pending_positions.json, so
                // nothing replays it); later settlements are unaffected. It never
                // double-counts on replay.
                if let Err(error) = account.persist(&log_dir).await {
                    tracing::error!("[STATE] failed to persist account risk state: {error:#}");
                    util::halt_on_state_write_failure(&persist_halt, &log_dir).await;
                }
                account.balance
            };
            if mode.is_paper() {
                util::write_balance(&log_dir, balance).await;
            }

            for (result, redeem_needed) in &settled {
                tui_state.write().await.settle_market(
                    &result.market_topic_id.to_string(),
                    result.won,
                    result.pnl,
                );
                trade_log.log_settlement(result, current_price).await;
                if *redeem_needed {
                    redeem_with_retry(&client, &result.token_id).await;
                }
            }
        }
    })
}

#[allow(clippy::too_many_arguments)]
async fn reconcile_pending_orders(
    mode: TradingMode,
    client: &BinancePredictionClient,
    settler: &Arc<RwLock<Settler>>,
    account: &Arc<RwLock<AccountState>>,
    trade_log: &TradeLogHandle,
    log_dir: &str,
    tui_state: &Arc<RwLock<TuiState>>,
    persist_halt: &Arc<AtomicBool>,
) {
    if mode.is_paper() {
        return;
    }
    // Bind the guard to a `let` so the read lock drops here; iterating the guard
    // directly would hold it across the `settler.write()` calls below and
    // deadlock the (non-reentrant) RwLock.
    let awaiting = settler.read().await.awaiting_reconciliation();
    for position in awaiting {
        match client
            .reconcile_order(&position.order_id, position.settlement_time_ms)
            .await
        {
            Ok(OrderReconciliation::Pending) => {}
            Ok(OrderReconciliation::Unfilled) => {
                tracing::warn!("[EXEC] Binance FOK {} was unfilled", position.order_id);
                settler
                    .write()
                    .await
                    .remove_unfilled(position.market_topic_id);
                if let Err(error) = settler.read().await.persist(log_dir).await {
                    tracing::error!("[STATE] failed to persist unfilled Binance order: {error:#}");
                    util::halt_on_state_write_failure(persist_halt, log_dir).await;
                }
            }
            Ok(OrderReconciliation::Filled(fill)) => {
                let entry_price = fill.entry_price();
                let order = OrderResult {
                    order_id: fill.order_id,
                    direction: position.direction,
                    requested_usdt: position.requested_usdt,
                    entry_price,
                    filled_shares: fill.filled_shares,
                    trade_cost: fill.trade_cost,
                    fee: fill.fee,
                    settlement_time_ms: position.settlement_time_ms,
                    entry_btc_price: position.entry_btc_price,
                };
                let updated = settler
                    .write()
                    .await
                    .mark_filled(position.market_topic_id, order.clone());
                let Some(updated) = updated else {
                    continue;
                };
                if let Err(error) = settler.read().await.persist(log_dir).await {
                    tracing::error!(
                        "[STATE] failed to persist reconciled Binance order: {error:#}"
                    );
                    util::halt_on_state_write_failure(persist_halt, log_dir).await;
                }
                let balance = {
                    let mut account = account.write().await;
                    account.record_trade(order.total_cost());
                    if let Err(error) = account.persist(log_dir).await {
                        tracing::error!("[STATE] failed to persist account risk state: {error:#}");
                        util::halt_on_state_write_failure(persist_halt, log_dir).await;
                    }
                    account.balance
                };
                trade_log
                    .log_reconciled_entry(&updated, &order, balance)
                    .await;
                tui_state.write().await.add_trade(TradeRow {
                    time: Utc::now(),
                    market_topic_id: updated.market_topic_id.to_string(),
                    direction: order.direction.as_str().to_string(),
                    entry_price: order.entry_price,
                    cost: order.total_cost(),
                    edge: Default::default(),
                    result: "PENDING".to_string(),
                    pnl: None,
                });
            }
            Err(error) => tracing::warn!(
                "[EXEC] failed to reconcile Binance order {}: {error:#}",
                position.order_id
            ),
        }
    }
}

/// Claim a winning position, retrying transient failures so a single RPC hiccup
/// does not strand real funds.
// ponytail: bounded inline retry, not a durable queue. A crash between settlement
// persist and a successful redeem still needs manual `binance-5m-tools --redeem`
// (Binance retains settled-position history). Add a persisted redeem queue only
// if crash-time redemption loss is observed in practice.
async fn redeem_with_retry(client: &BinancePredictionClient, token_id: &str) {
    const ATTEMPTS: usize = 3;
    for attempt in 1..=ATTEMPTS {
        match client.redeem(token_id).await {
            Ok(receipt) => {
                tracing::info!(
                    "[REDEEM] Binance token={} status={} tx={}",
                    token_id,
                    receipt.status,
                    receipt.transaction_hash.as_deref().unwrap_or("pending"),
                );
                return;
            }
            Err(error) if attempt < ATTEMPTS => {
                tracing::warn!(
                    "[REDEEM] Binance token={token_id} attempt {attempt}/{ATTEMPTS} failed, retrying: {error:#}"
                );
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            Err(error) => tracing::error!(
                "[REDEEM] Binance token={token_id} failed after {ATTEMPTS} attempts; use binance-5m-tools --redeem for recovery: {error:#}"
            ),
        }
    }
}

fn redemption_needed(status: Option<&str>) -> bool {
    !matches!(
        status.map(|status| status.to_ascii_uppercase()).as_deref(),
        Some("CONFIRMED" | "CLAIMED" | "PENDING")
    )
}

#[cfg(test)]
mod tests {
    use super::redemption_needed;

    #[test]
    fn redemption_needed_when_status_missing_or_unknown() {
        // No recorded status means the winning token has not yet been claimed; a redeem
        // request must still be submitted or real funds stay stranded on Binance.
        assert!(redemption_needed(None));
        assert!(redemption_needed(Some("")));
        assert!(redemption_needed(Some("NOT_REDEEMED")));
    }

    #[test]
    fn redemption_suppressed_for_terminal_and_pending_states() {
        // CONFIRMED / CLAIMED are terminal; PENDING means a redeem is already in flight.
        // All three must suppress a duplicate redeem request, case-insensitively.
        for status in ["CONFIRMED", "CLAIMED", "PENDING"] {
            assert!(
                !redemption_needed(Some(status)),
                "{status} should suppress redeem"
            );
        }
        assert!(!redemption_needed(Some("confirmed")));
        assert!(!redemption_needed(Some("Claimed")));
        assert!(!redemption_needed(Some("pending")));
    }
}
