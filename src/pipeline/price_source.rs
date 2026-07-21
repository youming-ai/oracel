//! Stage 1: Price Source — Optimized for 5min window latency
//!
//! Performance targets:
//! - <1ms price ingestion latency
//! - Lock-free read path for latest price
//! - Zero-allocation hot path

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::RwLock;

use crate::data::binance::BinanceClient;

use rust_decimal::{Decimal, MathematicalOps};

/// Uniform ticker update shared across all price source backends.
#[derive(Debug, Clone, Copy)]
struct TickerUpdate {
    price: Decimal,
    timestamp_ms: i64,
}

impl From<crate::data::binance::TickerUpdate> for TickerUpdate {
    fn from(t: crate::data::binance::TickerUpdate) -> Self {
        Self {
            price: t.price,
            timestamp_ms: t.timestamp_ms,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PriceTick {
    price: Decimal,
    timestamp_ms: i64,
}

pub struct PriceSource {
    client: Arc<BinanceClient>,
    buffer: Arc<RwLock<VecDeque<PriceTick>>>,
    max: usize,
}

pub struct PriceSourceHandles {
    pub ws_handle: tokio::task::JoinHandle<()>,
    pub receiver_handle: tokio::task::JoinHandle<()>,
}

impl PriceSource {
    pub fn new(symbol: &str, max: usize) -> Self {
        Self {
            client: Arc::new(BinanceClient::new(symbol)),
            buffer: Arc::new(RwLock::new(VecDeque::with_capacity(max))),
            max,
        }
    }

    #[inline]
    pub async fn latest(&self) -> Option<Decimal> {
        self.buffer.read().await.back().map(|t| t.price)
    }

    #[inline]
    pub async fn last_tick_ms(&self) -> Option<i64> {
        self.buffer.read().await.back().map(|t| t.timestamp_ms)
    }

    #[inline]
    pub async fn buffer_len(&self) -> usize {
        self.buffer.read().await.len()
    }

    /// Compute the BTC price trend as a percentage change over the last `window_s` seconds.
    /// Returns `None` if insufficient data is available.
    /// Positive = BTC rising, negative = BTC falling.
    pub async fn trend_pct(&self, window_s: u64) -> Option<Decimal> {
        let buf = self.buffer.read().await;
        if buf.is_empty() {
            return None;
        }
        let latest = buf.back()?;
        let cutoff_ms = latest.timestamp_ms - (window_s as i64 * 1000);
        // Find the earliest tick that is at or after the cutoff
        let old = buf
            .iter()
            .find(|t| t.timestamp_ms >= cutoff_ms)
            .or_else(|| buf.front())?;
        if old.price == Decimal::ZERO {
            return None;
        }
        Some((latest.price - old.price) / old.price * Decimal::from(100))
    }

    /// Estimate one-second realized volatility from Binance BTCUSDT returns.
    /// Gaps are normalized by elapsed time, so reconnects do not inflate sigma.
    pub async fn realized_sigma_per_second(
        &self,
        lookback_secs: u64,
        min_samples: usize,
    ) -> Option<Decimal> {
        let buffer = self.buffer.read().await;
        let latest = buffer.back()?;
        let lookback_ms = i64::try_from(lookback_secs).ok()?.saturating_mul(1_000);
        let cutoff_ms = latest.timestamp_ms.saturating_sub(lookback_ms);
        let ticks: Vec<_> = buffer
            .iter()
            .filter(|tick| tick.timestamp_ms >= cutoff_ms)
            .collect();
        if ticks.len() <= min_samples {
            return None;
        }
        let mut squared_returns = Decimal::ZERO;
        let mut elapsed_ms = 0_i64;
        let mut samples = 0_usize;
        for pair in ticks.windows(2) {
            let interval_ms = pair[1].timestamp_ms.saturating_sub(pair[0].timestamp_ms);
            if pair[0].price <= Decimal::ZERO || interval_ms <= 0 {
                continue;
            }
            let price_return = (pair[1].price - pair[0].price) / pair[0].price;
            squared_returns += price_return * price_return;
            elapsed_ms = elapsed_ms.saturating_add(interval_ms);
            samples += 1;
        }
        if samples < min_samples || elapsed_ms <= 0 {
            return None;
        }
        let elapsed_seconds = Decimal::from(elapsed_ms) / Decimal::from(1_000);
        (squared_returns / elapsed_seconds).sqrt()
    }

    pub async fn start(&self, shutdown: Arc<AtomicBool>) -> PriceSourceHandles {
        let ws_client = self.client.clone();
        let ws_handle = tokio::spawn(async move {
            if let Err(e) = ws_client.start_ticker_ws().await {
                tracing::error!("[WS] Binance WS stopped: {}", e);
            }
        });
        let receiver_handle = Self::spawn_receiver(
            self.buffer.clone(),
            self.max,
            self.client.subscribe(),
            "Binance",
            shutdown,
        );
        PriceSourceHandles {
            ws_handle,
            receiver_handle,
        }
    }

    fn spawn_receiver<T: Into<TickerUpdate> + Clone + Send + 'static>(
        buf: Arc<RwLock<VecDeque<PriceTick>>>,
        max: usize,
        mut rx: broadcast::Receiver<T>,
        source: &'static str,
        shutdown: Arc<AtomicBool>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                if shutdown.load(Ordering::Acquire) {
                    break;
                }

                match rx.recv().await {
                    Ok(raw) => {
                        let ticker: TickerUpdate = raw.into();
                        let mut h = buf.write().await;
                        if h.back()
                            .map(|last| ticker.timestamp_ms >= last.timestamp_ms)
                            .unwrap_or(true)
                        {
                            h.push_back(PriceTick {
                                price: ticker.price,
                                timestamp_ms: ticker.timestamp_ms,
                            });
                            if h.len() > max {
                                h.pop_front();
                            }
                        } else {
                            tracing::debug!(
                                "[WS] Ignoring out-of-order {} tick ts={} < {}",
                                source,
                                ticker.timestamp_ms,
                                h.back().map(|last| last.timestamp_ms).unwrap_or(0)
                            );
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::info!("[WS] Price receiver lagged by {} messages", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::error!("[WS] {} price channel closed", source);
                        break;
                    }
                }
            }
        })
    }
}
