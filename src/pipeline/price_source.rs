//! Stage 1: Price Source — BTC price buffer from Coinbase WS.

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::data::coinbase::CoinbaseClient;

#[derive(Debug, Clone)]
pub(crate) struct PriceTick {
    pub price: f64,
    pub timestamp_ms: i64,
}

pub(crate) struct PriceSource {
    client: Arc<CoinbaseClient>,
    buffer: Arc<RwLock<VecDeque<PriceTick>>>,
    max: usize,
}

impl PriceSource {
    pub(crate) fn new(client: Arc<CoinbaseClient>, max: usize) -> Self {
        Self {
            client,
            buffer: Arc::new(RwLock::new(VecDeque::with_capacity(max))),
            max,
        }
    }

    pub(crate) async fn latest(&self) -> Option<f64> {
        self.buffer.read().await.back().map(|t| t.price)
    }

    pub(crate) async fn last_tick_ms(&self) -> Option<i64> {
        self.buffer.read().await.back().map(|t| t.timestamp_ms)
    }

    pub(crate) async fn history(&self) -> Vec<PriceTick> {
        self.buffer.read().await.iter().cloned().collect()
    }

    pub(crate) async fn start(self: Arc<Self>) {
        let client_for_ws = self.client.clone();
        tokio::spawn(async move {
            if let Err(e) = client_for_ws.start_ticker_ws().await {
                tracing::error!("[WS] Coinbase WS failed: {}", e);
            }
        });

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let mut rx = self.client.subscribe();
        let buf = self.buffer.clone();
        let max = self.max;
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(ticker) => {
                        let tick = PriceTick {
                            price: ticker.price,
                            timestamp_ms: chrono::Utc::now().timestamp_millis(),
                        };
                        let mut h = buf.write().await;
                        h.push_back(tick);
                        if h.len() > max {
                            h.pop_front();
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::debug!("[WS] Price receiver lagged by {} messages", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        tracing::error!("[WS] Price channel closed");
                        break;
                    }
                }
            }
        });
    }
}
