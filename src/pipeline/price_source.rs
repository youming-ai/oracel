//! Stage 1: Price Source — Optimized for 5min window latency
//!
//! Performance targets:
//! - <1ms price ingestion latency
//! - Lock-free read path for latest price
//! - Zero-allocation hot path

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::PriceSourceType;
use crate::data::binance::BinanceClient;
use crate::data::coinbase::CoinbaseClient;

#[derive(Debug, Clone, Copy)]
pub(crate) struct PriceTick {
    pub price: f64,
    pub timestamp_ms: i64,
}

pub(crate) enum PriceClient {
    Binance(Arc<BinanceClient>),
    Coinbase(Arc<CoinbaseClient>),
}

pub(crate) struct PriceSource {
    client: PriceClient,
    buffer: Arc<RwLock<VecDeque<PriceTick>>>,
    max: usize,
    started: std::sync::atomic::AtomicBool,
}

impl PriceSource {
    pub(crate) fn new(source_type: PriceSourceType, symbol: &str, max: usize) -> Self {
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
    pub(crate) async fn latest(&self) -> Option<f64> {
        self.buffer.read().await.back().map(|t| t.price)
    }

    #[inline]
    pub(crate) async fn last_tick_ms(&self) -> Option<i64> {
        self.buffer.read().await.back().map(|t| t.timestamp_ms)
    }

    pub(crate) async fn history(&self) -> Vec<PriceTick> {
        self.buffer.read().await.iter().copied().collect()
    }

    pub(crate) async fn start(self: Arc<Self>) {
        if self.started.swap(true, std::sync::atomic::Ordering::SeqCst) {
            tracing::warn!("[PRICE] PriceSource already started, skipping");
            return;
        }

        match &self.client {
            PriceClient::Binance(client) => {
                self.spawn_binance_tasks(client.clone());
            }
            PriceClient::Coinbase(client) => {
                self.spawn_coinbase_tasks(client.clone());
            }
        }
    }

    fn spawn_binance_tasks(&self, client: Arc<BinanceClient>) {
        let client_for_ws = client.clone();
        tokio::spawn(async move {
            if let Err(e) = client_for_ws.start_ticker_ws().await {
                tracing::error!("[WS] Binance WS stopped: {}", e);
            }
        });

        let buf = self.buffer.clone();
        let max = self.max;
        let mut rx = client.subscribe();

        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(ticker) => {
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
                                "[WS] Ignoring out-of-order Binance tick ts={} < {}",
                                ticker.timestamp_ms,
                                h.back().map(|last| last.timestamp_ms).unwrap_or(0)
                            );
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::debug!("[WS] Price receiver lagged by {} messages", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        tracing::error!("[WS] Binance price channel closed");
                        break;
                    }
                }
            }
        });
    }

    fn spawn_coinbase_tasks(&self, client: Arc<CoinbaseClient>) {
        let client_for_ws = client.clone();
        tokio::spawn(async move {
            if let Err(e) = client_for_ws.start_ticker_ws().await {
                tracing::error!("[WS] Coinbase WS stopped: {}", e);
            }
        });

        let buf = self.buffer.clone();
        let max = self.max;
        let mut rx = client.subscribe();

        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(ticker) => {
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
                                "[WS] Ignoring out-of-order Coinbase tick ts={} < {}",
                                ticker.timestamp_ms,
                                h.back().map(|last| last.timestamp_ms).unwrap_or(0)
                            );
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::debug!("[WS] Price receiver lagged by {} messages", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        tracing::error!("[WS] Coinbase price channel closed");
                        break;
                    }
                }
            }
        });
    }
}
