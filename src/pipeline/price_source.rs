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

use crate::config::PriceSourceType;
use crate::data::binance::BinanceClient;
use crate::data::coinbase::CoinbaseClient;

use rust_decimal::Decimal;

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

impl From<crate::data::coinbase::TickerUpdate> for TickerUpdate {
    fn from(t: crate::data::coinbase::TickerUpdate) -> Self {
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

pub struct SpotMomentumSnapshot {
    pub latest: Decimal,
    pub price_30s_ago: Option<Decimal>,
    pub price_60s_ago: Option<Decimal>,
}

pub enum PriceClient {
    Binance(Arc<BinanceClient>),
    Coinbase(Arc<CoinbaseClient>),
}

pub struct PriceSource {
    client: PriceClient,
    buffer: Arc<RwLock<VecDeque<PriceTick>>>,
    max: usize,
    started: std::sync::atomic::AtomicBool,
}

pub struct PriceSourceHandles {
    pub ws_handle: tokio::task::JoinHandle<()>,
    pub receiver_handle: tokio::task::JoinHandle<()>,
}

impl PriceSource {
    pub fn new(source_type: PriceSourceType, symbol: &str, max: usize) -> Self {
        let client = match source_type {
            PriceSourceType::Binance | PriceSourceType::BinanceWs => {
                PriceClient::Binance(Arc::new(BinanceClient::new(symbol)))
            }
            PriceSourceType::Coinbase | PriceSourceType::CoinbaseWs => {
                PriceClient::Coinbase(Arc::new(CoinbaseClient::new(symbol)))
            }
        };

        Self {
            client,
            buffer: Arc::new(RwLock::new(VecDeque::with_capacity(max))),
            max,
            started: std::sync::atomic::AtomicBool::new(false),
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
    pub async fn momentum_snapshot(&self) -> Option<SpotMomentumSnapshot> {
        let buffer = self.buffer.read().await;
        let latest_tick = *buffer.back()?;

        let cutoff_30s = latest_tick.timestamp_ms - 30_000;
        let cutoff_60s = latest_tick.timestamp_ms - 60_000;

        let price_30s_ago = buffer
            .iter()
            .rev()
            .find(|tick| tick.timestamp_ms <= cutoff_30s)
            .map(|tick| tick.price);
        let price_60s_ago = buffer
            .iter()
            .rev()
            .find(|tick| tick.timestamp_ms <= cutoff_60s)
            .map(|tick| tick.price);

        Some(SpotMomentumSnapshot {
            latest: latest_tick.price,
            price_30s_ago,
            price_60s_ago,
        })
    }

    pub async fn buffer_len(&self) -> usize {
        self.buffer.read().await.len()
    }

    pub async fn start(self: Arc<Self>, shutdown: Arc<AtomicBool>) -> PriceSourceHandles {
        if self.started.swap(true, Ordering::SeqCst) {
            tracing::warn!("[PRICE] PriceSource already started, skipping");
            return PriceSourceHandles {
                ws_handle: tokio::spawn(async {}),
                receiver_handle: tokio::spawn(async {}),
            };
        }

        match &self.client {
            PriceClient::Binance(client) => {
                let ws_client = client.clone();
                let ws_handle = tokio::spawn(async move {
                    if let Err(e) = ws_client.start_ticker_ws().await {
                        tracing::error!("[WS] Binance WS stopped: {}", e);
                    }
                });
                let receiver_handle = Self::spawn_receiver(
                    self.buffer.clone(),
                    self.max,
                    client.subscribe(),
                    "Binance",
                    shutdown.clone(),
                );
                PriceSourceHandles {
                    ws_handle,
                    receiver_handle,
                }
            }
            PriceClient::Coinbase(client) => {
                let ws_client = client.clone();
                let ws_handle = tokio::spawn(async move {
                    if let Err(e) = ws_client.start_ticker_ws().await {
                        tracing::error!("[WS] Coinbase WS stopped: {}", e);
                    }
                });
                let receiver_handle = Self::spawn_receiver(
                    self.buffer.clone(),
                    self.max,
                    client.subscribe(),
                    "Coinbase",
                    shutdown,
                );
                PriceSourceHandles {
                    ws_handle,
                    receiver_handle,
                }
            }
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
                if shutdown.load(Ordering::Relaxed) {
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
                        tracing::debug!("[WS] Price receiver lagged by {} messages", n);
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

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;

    use super::{PriceSource, PriceSourceType, PriceTick};

    fn d(value: &str) -> Decimal {
        Decimal::from_str_exact(value).expect("valid decimal")
    }

    fn test_source(max: usize) -> PriceSource {
        PriceSource::new(PriceSourceType::Binance, "BTCUSDT", max)
    }

    async fn push_tick(source: &PriceSource, price: Decimal, timestamp_ms: i64) {
        source.buffer.write().await.push_back(PriceTick {
            price,
            timestamp_ms,
        });
    }

    #[tokio::test]
    async fn test_momentum_snapshot_returns_latest_and_lookbacks() {
        let source = test_source(16);
        push_tick(&source, d("100.0"), 1_000).await;
        push_tick(&source, d("101.0"), 31_000).await;
        push_tick(&source, d("102.0"), 61_000).await;

        let snapshot = source
            .momentum_snapshot()
            .await
            .expect("snapshot should exist");

        assert_eq!(snapshot.latest, d("102.0"));
        assert_eq!(snapshot.price_30s_ago, Some(d("101.0")));
        assert_eq!(snapshot.price_60s_ago, Some(d("100.0")));
    }

    #[tokio::test]
    async fn test_momentum_snapshot_returns_none_for_missing_60s_history() {
        let source = test_source(16);
        push_tick(&source, d("200.0"), 15_000).await;
        push_tick(&source, d("201.0"), 45_000).await;

        let snapshot = source
            .momentum_snapshot()
            .await
            .expect("snapshot should exist");

        assert_eq!(snapshot.latest, d("201.0"));
        assert_eq!(snapshot.price_30s_ago, Some(d("200.0")));
        assert_eq!(snapshot.price_60s_ago, None);
    }

    #[tokio::test]
    async fn test_momentum_snapshot_uses_nearest_older_tick() {
        let source = test_source(16);
        push_tick(&source, d("300.0"), 10_000).await;
        push_tick(&source, d("301.0"), 29_500).await;
        push_tick(&source, d("302.0"), 40_000).await;
        push_tick(&source, d("303.0"), 69_000).await;

        let snapshot = source
            .momentum_snapshot()
            .await
            .expect("snapshot should exist");

        assert_eq!(snapshot.latest, d("303.0"));
        assert_eq!(snapshot.price_30s_ago, Some(d("301.0")));
        assert_eq!(snapshot.price_60s_ago, None);
    }

    #[tokio::test]
    async fn test_momentum_snapshot_handles_irregular_tick_spacing() {
        let source = test_source(16);
        push_tick(&source, d("400.0"), 1_000).await;
        push_tick(&source, d("401.0"), 1_500).await;
        push_tick(&source, d("402.0"), 20_000).await;
        push_tick(&source, d("403.0"), 45_000).await;
        push_tick(&source, d("404.0"), 120_000).await;

        let snapshot = source
            .momentum_snapshot()
            .await
            .expect("snapshot should exist");

        assert_eq!(snapshot.latest, d("404.0"));
        assert_eq!(snapshot.price_30s_ago, Some(d("403.0")));
        assert_eq!(snapshot.price_60s_ago, Some(d("403.0")));
    }
}
