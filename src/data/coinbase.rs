//! Coinbase real-time BTC price client (Advanced Trade WebSocket)

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const WS_URL: &str = "wss://advanced-trade-ws.coinbase.com";
const PRICE_CHANNEL_BUFFER: usize = 1000;

#[derive(Debug, Clone)]
pub(crate) struct TickerUpdate {
    pub price: f64,
}

pub(crate) struct CoinbaseClient {
    product_id: String,
    price_tx: broadcast::Sender<TickerUpdate>,
    latest_price: Arc<RwLock<Option<f64>>>,
}

impl CoinbaseClient {
    pub(crate) fn new(product_id: &str) -> Self {
        let (price_tx, _) = broadcast::channel(PRICE_CHANNEL_BUFFER);
        Self {
            product_id: product_id.to_string(),
            price_tx,
            latest_price: Arc::new(RwLock::new(None)),
        }
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<TickerUpdate> {
        self.price_tx.subscribe()
    }

    pub(crate) async fn start_ticker_ws(self: Arc<Self>) -> Result<()> {
        tracing::info!("[WS] connecting {}", WS_URL);
        let mut backoff_secs: u64 = 1;
        const MAX_BACKOFF_SECS: u64 = 60;

        loop {
            match self.ws_loop().await {
                Ok(_) => {
                    tracing::warn!("WS disconnected, reconnecting...");
                    backoff_secs = 1;
                }
                Err(e) => {
                    tracing::error!("WS error: {}, reconnecting in {}s...", e, backoff_secs);
                }
            }
            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
            backoff_secs = (backoff_secs * 2).min(MAX_BACKOFF_SECS);
        }
    }

    async fn ws_loop(&self) -> Result<()> {
        let (ws, _) = tokio::time::timeout(Duration::from_secs(10), connect_async(WS_URL))
            .await
            .map_err(|_| anyhow::anyhow!("WS connect timed out after 10s"))?
            .context("WS connect failed")?;
        let (mut write, mut read) = ws.split();

        let subscribe_msg = serde_json::json!({
            "type": "subscribe",
            "product_ids": [self.product_id],
            "channel": "ticker"
        });
        write
            .send(Message::Text(subscribe_msg.to_string()))
            .await
            .context("Failed to subscribe")?;

        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => self.handle_message(&text),
                Ok(Message::Ping(data)) => {
                    if let Err(e) = write.send(Message::Pong(data)).await {
                        tracing::debug!("[WS] pong send failed: {}", e);
                    }
                }
                Ok(Message::Close(_)) => break,
                Err(e) => {
                    tracing::warn!("WS error: {}", e);
                    break;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn handle_message(&self, text: &str) {
        if let Ok(root) = serde_json::from_str::<serde_json::Value>(text) {
            if root.get("channel").and_then(|v| v.as_str()) != Some("ticker") {
                return;
            }
            if let Some(events) = root.get("events").and_then(|v| v.as_array()) {
                for event in events {
                    if let Some(tickers) = event.get("tickers").and_then(|v| v.as_array()) {
                        for ticker in tickers {
                            let pid = ticker
                                .get("product_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            if pid != self.product_id {
                                continue;
                            }
                            if let Some(price) = ticker
                                .get("price")
                                .and_then(|v| v.as_str())
                                .and_then(|s| s.parse::<f64>().ok())
                            {
                                if let Ok(mut guard) = self.latest_price.try_write() {
                                    *guard = Some(price);
                                }
                                if self.price_tx.send(TickerUpdate { price }).is_err() {
                                    tracing::debug!("[WS] no price receivers");
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
