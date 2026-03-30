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

    pub async fn buffer_len(&self) -> usize {
        self.buffer.read().await.len()
    }

    /// Compute price momentum as percentage change over the given time window.
    /// Positive = price went up, negative = price went down.
    /// Returns None if buffer has insufficient data.
    pub async fn momentum_pct(&self, window_secs: i64) -> Option<Decimal> {
        let buf = self.buffer.read().await;
        if buf.len() < 2 {
            return None;
        }
        let latest = buf.back()?;
        let cutoff_ms = latest.timestamp_ms - window_secs * 1000;

        // Find earliest tick at or past the cutoff (buffer is time-ordered)
        let idx = buf.iter().position(|t| t.timestamp_ms >= cutoff_ms)?;
        let old = &buf[idx];

        if old.price <= Decimal::ZERO {
            return None;
        }

        Some((latest.price - old.price) / old.price)
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
