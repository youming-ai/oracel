//! Coinbase real-time BTC price client (Advanced Trade WebSocket)

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const WS_URL: &str = "wss://advanced-trade-ws.coinbase.com";

#[derive(Debug, Clone)]
pub struct TickerUpdate {
    pub price: f64,
}

pub struct CoinbaseClient {
    product_id: String,
    price_tx: broadcast::Sender<TickerUpdate>,
    latest_price: Arc<RwLock<Option<f64>>>,
}

impl CoinbaseClient {
    pub fn new(product_id: &str) -> Self {
        let (price_tx, _) = broadcast::channel(1000);
        Self {
            product_id: product_id.to_string(),
            price_tx,
            latest_price: Arc::new(RwLock::new(None)),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TickerUpdate> {
        self.price_tx.subscribe()
    }

    pub async fn start_ticker_ws(self: Arc<Self>) -> Result<()> {
        tracing::info!("[WS] connecting {}", WS_URL);
        loop {
            match self.ws_loop().await {
                Ok(_) => tracing::warn!("WS disconnected, reconnecting..."),
                Err(e) => tracing::error!("WS error: {}, reconnecting...", e),
            }
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
    }

    async fn ws_loop(&self) -> Result<()> {
        let (ws, _) = connect_async(WS_URL).await.context("WS connect failed")?;
        let (mut write, mut read) = ws.split();

        let subscribe_msg = serde_json::json!({
            "type": "subscribe",
            "product_ids": [self.product_id],
            "channel": "ticker"
        });
        write
            .send(Message::Text(subscribe_msg.to_string().into()))
            .await
            .context("Failed to subscribe")?;

        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => self.handle_message(&text),
                Ok(Message::Ping(data)) => { let _ = write.send(Message::Pong(data)).await; }
                Ok(Message::Close(_)) => break,
                Err(e) => { tracing::warn!("WS error: {}", e); break; }
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
                            let pid = ticker.get("product_id").and_then(|v| v.as_str()).unwrap_or("");
                            if pid != self.product_id { continue; }
                            if let Some(price) = ticker.get("price").and_then(|v| v.as_str()).and_then(|s| s.parse::<f64>().ok()) {
                                if let Ok(mut guard) = self.latest_price.try_write() {
                                    *guard = Some(price);
                                }
                                let _ = self.price_tx.send(TickerUpdate { price });
                            }
                        }
                    }
                }
            }
        }
    }
}
