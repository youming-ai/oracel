//! Stage 1: Price Source
//! Collects BTC price from Coinbase WS, maintains rolling buffer.

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use crate::data::coinbase::CoinbaseClient;

#[derive(Debug, Clone)]
pub struct PriceTick {
    pub price: f64,
    pub ts_ms: i64,
}

pub struct PriceSource {
    client: Arc<CoinbaseClient>,
    buffer: Arc<RwLock<VecDeque<PriceTick>>>,
    max: usize,
    tx: broadcast::Sender<PriceTick>,
}

impl PriceSource {
    pub fn new(client: Arc<CoinbaseClient>, max: usize) -> Self {
        let (tx, _) = broadcast::channel(1000);
        Self { client, buffer: Arc::new(RwLock::new(VecDeque::with_capacity(max))), max, tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<PriceTick> { self.tx.subscribe() }

    pub async fn latest(&self) -> Option<f64> {
        self.buffer.read().await.back().map(|t| t.price)
    }

    pub async fn history(&self) -> Vec<PriceTick> {
        self.buffer.read().await.iter().cloned().collect()
    }

    pub async fn change_pct(&self, lookback: usize) -> Option<f64> {
        let h = self.buffer.read().await;
        if h.len() < lookback + 1 { return None; }
        let cur = h.back()?.price;
        let past = h.get(h.len() - 1 - lookback)?.price;
        if past <= 0.0 { return None; }
        Some((cur - past) / past)
    }

    pub async fn sma(&self, period: usize) -> Option<f64> {
        let h = self.buffer.read().await;
        if h.len() < period { return None; }
        let sum: f64 = h.iter().rev().take(period).map(|t| t.price).sum();
        Some(sum / period as f64)
    }
    /// Start Coinbase WS + collect prices into buffer
    pub async fn start(self: Arc<Self>) {
        // Start the Coinbase WS first
        let client_for_ws = self.client.clone();
        tokio::spawn(async move {
            if let Err(e) = client_for_ws.start_ticker_ws().await {
                tracing::error!("Coinbase WS failed: {}", e);
            }
        });

        // Wait for WS to connect
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Now collect prices
        let mut rx = self.client.subscribe();
        let buf = self.buffer.clone();
        let tx = self.tx.clone();
        let max = self.max;
        tokio::spawn(async move {
            loop {
                while let Ok(ticker) = rx.try_recv() {
                    let tick = PriceTick { price: ticker.price, ts_ms: chrono::Utc::now().timestamp_millis() };
                    let mut h = buf.write().await;
                    h.push_back(tick.clone());
                    if h.len() > max { h.pop_front(); }
                    let _ = tx.send(tick);
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        });
    }

    pub async fn len(&self) -> usize { self.buffer.read().await.len() }
}
