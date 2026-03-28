use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::Utc;
use futures_util::future::join_all;
use polymarket_5m_bot::config;
use polymarket_5m_bot::data::market_discovery::{
    infer_resolution_state, MarketDiscovery, ResolutionState,
};
use polymarket_5m_bot::data::polymarket::CtfRedeemer;
use polymarket_5m_bot::pipeline::decider::AccountState;
use polymarket_5m_bot::pipeline::price_source::PriceSource;
use polymarket_5m_bot::pipeline::settler::Settler;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use tokio::sync::RwLock;
use tokio::time::Duration;

use crate::state::MarketState;

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

pub(crate) fn start_market_refresher(
    discovery: Arc<MarketDiscovery>,
    market_state: Arc<RwLock<MarketState>>,
    shutdown: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            if shutdown.load(Ordering::Acquire) {
                tracing::info!("[TASK] market refresher shutting down");
                break;
            }

            interval.tick().await;
            match discovery.discover().await {
                Ok(active) => {
                    let current_yes = market_state.read().await.token_yes.clone();
                    if current_yes != active.token_id_yes.clone().into() {
                        tracing::info!("[MKT] {} ends {}", active.market.slug, active.end_date);
                        *market_state.write().await = MarketState {
                            token_yes: active.token_id_yes.into(),
                            token_no: active.token_id_no.into(),
                            condition_id: active.condition_id.into(),
                            market_slug: active.market.slug.into(),
                            settlement_ms: active.end_date.timestamp_millis(),
                        };
                    }
                }
                Err(e) => {
                    tracing::debug!("[MARKET] Market refresh failed: {}", e);
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
    mode: config::TradingMode,
    status_interval_ms: u64,
    shutdown: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(status_interval_ms));
        loop {
            if shutdown.load(Ordering::Acquire) {
                tracing::info!("[TASK] status printer shutting down");
                break;
            }

            interval.tick().await;

            let btc = price_source.latest().await.unwrap_or(Decimal::ZERO);
            let acc = account.read().await;
            let pending = settler.read().await.pending_count();
            let settle = market_state.read().await.settlement_ms;

            let ttl = if settle > 0 {
                let remaining_s = (settle - Utc::now().timestamp_millis()).max(0) / 1000;
                if remaining_s > 0 {
                    format!("{}m{}s", remaining_s / 60, remaining_s % 60)
                } else {
                    "expired".into()
                }
            } else {
                "?".into()
            };

            let pnl = acc.pnl();
            tracing::info!(
                "[STATUS] {} | BTC=${:.0} | bal=${:.2} pnl={:+.2} | {}W/{}L streak={} | pending={} | ttl={}",
                mode,
                btc.to_f64().unwrap_or(0.0),
                acc.balance,
                pnl,
                acc.total_wins,
                acc.total_losses,
                if acc.consecutive_wins > 0 {
                    format!("+{}", acc.consecutive_wins)
                } else if acc.consecutive_losses > 0 {
                    format!("-{}", acc.consecutive_losses)
                } else {
                    "0".into()
                },
                pending,
                ttl,
            );
        }
    })
}

pub(crate) fn start_settlement_checker(
    settler: Arc<RwLock<Settler>>,
    account: Arc<RwLock<AccountState>>,
    price_source: Arc<PriceSource>,
    discovery: Arc<MarketDiscovery>,
    redeemer: Option<Arc<CtfRedeemer>>,
    log_dir: String,
    shutdown: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        let mut redeem_queue: Vec<(String, String, u32)> = Vec::new();
        let mut pending_retries: HashMap<String, u32> = HashMap::new();

        loop {
            if shutdown.load(Ordering::Acquire) {
                tracing::info!("[TASK] settlement checker shutting down");
                break;
            }

            interval.tick().await;

            let mut results = Vec::new();
            let due = settler.read().await.due_positions();

            let fetch_futures = due.iter().map(|pos| async {
                let result = discovery.fetch_market_by_slug(&pos.market_slug).await;
                (pos.clone(), result)
            });
            let fetch_results = join_all(fetch_futures).await;

            for (pos, market_result) in fetch_results {
                let slug = pos.market_slug.to_string();
                let market = match market_result {
                    Ok(m) => m,
                    Err(e) => {
                        let retries = pending_retries.entry(slug).or_insert(0);
                        *retries = retries.saturating_add(1);
                        if *retries == 1 || (*retries).is_multiple_of(20) {
                            tracing::warn!(
                                "[SETTLE] Gamma fetch failed for {} (attempt {}): {}",
                                pos.market_slug,
                                retries,
                                e
                            );
                        }
                        continue;
                    }
                };

                match infer_resolution_state(&market) {
                    Some(ResolutionState::Resolved(winner)) => {
                        tracing::info!(
                            "[SETTLE] {} resolved -> {} won",
                            pos.market_slug,
                            winner.as_str(),
                        );
                        let won = pos.direction == winner;
                        if let Some(result) =
                            settler.write().await.settle_by_slug(&pos.market_slug, won)
                        {
                            results.push(result);
                        }
                        pending_retries.remove(&slug);
                    }
                    Some(ResolutionState::Pending) => {
                        let retries = pending_retries.entry(slug).or_insert(0);
                        *retries = retries.saturating_add(1);
                        if *retries == 1 || (*retries).is_multiple_of(20) {
                            tracing::warn!(
                                "[SETTLE] {} still pending after {}s",
                                pos.market_slug,
                                *retries * 15,
                            );
                        }
                    }
                    None => {
                        let retries = pending_retries.entry(slug).or_insert(0);
                        *retries = retries.saturating_add(1);
                        if *retries == 1 || (*retries).is_multiple_of(20) {
                            tracing::warn!(
                                "[SETTLE] resolution unclear for {} (attempt {})",
                                pos.market_slug,
                                retries,
                            );
                        }
                    }
                }
            }
            let settlement_btc_price = price_source.latest().await;

            if !results.is_empty() {
                let mut acc = account.write().await;
                let today = Utc::now().format("%Y-%m-%d").to_string();
                acc.reset_daily_if_needed(&today);
                for r in &results {
                    acc.record_settlement(r);
                }

                tracing::info!(
                    "[BAL] ${:.2} | {}W/{}L | settled={}",
                    acc.balance,
                    acc.total_wins,
                    acc.total_losses,
                    results.len(),
                );

                let bal = acc.balance;
                drop(acc);

                if let Some(btc_price) = settlement_btc_price {
                    let mut log_lines = String::new();
                    for r in &results {
                        log_lines.push_str(&format!(
                            "{},{},{},{:+.2},{:.0},{:.0}\n",
                            Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
                            if r.won { "WIN" } else { "LOSS" },
                            r.direction.as_str(),
                            r.pnl.round_dp(2),
                            r.entry_btc_price,
                            btc_price.to_f64().unwrap_or(0.0),
                        ));
                    }
                    let trades_path = Path::new(&log_dir).join("trades.csv");
                    match tokio::task::spawn_blocking(move || -> std::io::Result<()> {
                        use std::io::Write;

                        let mut file = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(trades_path)?;
                        file.write_all(log_lines.as_bytes())
                    })
                    .await
                    {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => tracing::warn!("[LOG] trades.csv write failed: {}", e),
                        Err(e) => tracing::warn!("[LOG] trades.csv task failed: {}", e),
                    }
                }

                write_balance(&log_dir, bal).await;

                for r in &results {
                    if r.won && !r.condition_id.is_empty() {
                        redeem_queue.push((
                            r.condition_id.clone(),
                            r.direction.as_str().to_string(),
                            10,
                        ));
                    }
                }
            }

            if let Some(ref redeemer) = redeemer {
                let mut still_pending = Vec::new();
                for (cid, dir, attempts) in redeem_queue.drain(..) {
                    match redeemer.has_redeemable_position(&cid).await {
                        Ok(true) => match redeemer.redeem(&cid).await {
                            Ok(tx) => {
                                tracing::info!("[REDEEM] {} tx={}", dir, tx);
                            }
                            Err(e) => {
                                tracing::warn!("[REDEEM] {} failed: {}", dir, e);
                            }
                        },
                        Ok(false) if attempts > 1 => {
                            tracing::debug!(
                                "[REDEEM] {} not redeemable yet, {} retries left",
                                dir,
                                attempts - 1
                            );
                            still_pending.push((cid, dir, attempts - 1));
                        }
                        Ok(false) => {
                            tracing::debug!("[REDEEM] {} no redeemable position, dropping", dir);
                        }
                        Err(e) if attempts > 1 => {
                            tracing::debug!(
                                "[REDEEM] {} check failed: {}, {} retries left",
                                dir,
                                e,
                                attempts - 1
                            );
                            still_pending.push((cid, dir, attempts - 1));
                        }
                        Err(e) => {
                            tracing::warn!("[REDEEM] {} check failed, dropping: {}", dir, e);
                        }
                    }
                }
                redeem_queue = still_pending;
            }
        }
    })
}
