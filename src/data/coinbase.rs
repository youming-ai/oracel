//! Coinbase real-time BTC price client
//!
//! Uses Advanced Trade WebSocket (public, no auth needed for ticker).
//! Endpoint: wss://advanced-trade-ws.coinbase.com
//! Docs: https://docs.cdp.coinbase.com/coinbase-app/advanced-trade-apis/guides/websocket

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const WS_URL: &str = "wss://advanced-trade-ws.coinbase.com";
const REST_URL: &str = "https://api.coinbase.com/v2";

#[derive(Debug, Clone)]
pub struct TickerUpdate {
    pub price: f64,
    pub timestamp: i64,
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

    /// REST fallback for current BTC-USD price
    pub async fn fetch_price(&self) -> Result<f64> {
        let url = format!("{}/prices/{}/spot", REST_URL, self.product_id);
        let resp = reqwest::get(&url)
            .await
            .context("Coinbase REST request failed")?
            .json::<SpotPriceResp>()
            .await?;
        Ok(resp.data.amount.parse()?)
    }

    /// Start Advanced Trade WebSocket ticker stream
    pub async fn start_ticker_ws(self: Arc<Self>) -> Result<()> {
        tracing::info!("Coinbase Advanced Trade WS: {}", WS_URL);

        loop {
            match self.ws_loop().await {
                Ok(_) => tracing::warn!("Coinbase WS disconnected, reconnecting in 3s..."),
                Err(e) => tracing::error!("Coinbase WS error: {}, reconnecting in 5s...", e),
            }
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
    }

    async fn ws_loop(&self) -> Result<()> {
        let (ws, _) = connect_async(WS_URL).await.context("WS connect failed")?;
        let (mut write, mut read) = ws.split();

        // Subscribe to ticker channel (public, no auth)
        let subscribe_msg = serde_json::json!({
            "type": "subscribe",
            "product_ids": [self.product_id],
            "channel": "ticker"
        });
        write
            .send(Message::Text(subscribe_msg.to_string().into()))
            .await
            .context("Failed to subscribe")?;

        tracing::info!("Subscribed to ticker for {}", self.product_id);

        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    self.handle_message(&text);
                }
                Ok(Message::Ping(data)) => {
                    let _ = write.send(Message::Pong(data)).await;
                }
                Ok(Message::Close(_)) => break,
                Err(e) => {
                    tracing::warn!("WS message error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn handle_message(&self, text: &str) {
        // Advanced Trade WS format:
        // {"channel":"ticker","events":[{"type":"update","tickers":[{"product_id":"BTC-USD","price":"74230.50",...}]}]}
        if let Ok(root) = serde_json::from_str::<serde_json::Value>(text) {
            let channel = root.get("channel").and_then(|v| v.as_str()).unwrap_or("");
            if channel != "ticker" {
                return;
            }

            if let Some(events) = root.get("events").and_then(|v| v.as_array()) {
                for event in events {
                    if let Some(tickers) = event.get("tickers").and_then(|v| v.as_array()) {
                        for ticker in tickers {
                            // Match our product_id
                            let pid = ticker.get("product_id").and_then(|v| v.as_str()).unwrap_or("");
                            if pid != self.product_id {
                                continue;
                            }

                            if let Some(price_str) = ticker.get("price").and_then(|v| v.as_str()) {
                                if let Ok(price) = price_str.parse::<f64>() {
                                    if let Ok(mut guard) = self.latest_price.try_write() {
                                        *guard = Some(price);
                                    }
                                    let update = TickerUpdate {
                                        price,
                                        timestamp: chrono::Utc::now().timestamp_millis(),
                                    };
                                    let _ = self.price_tx.send(update);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct SpotPriceResp {
    data: SpotPriceData,
}

#[derive(Debug, Deserialize)]
struct SpotPriceData {
    amount: String,
}
