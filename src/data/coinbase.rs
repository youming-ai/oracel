//! Coinbase real-time BTC price client (Advanced Trade WebSocket)

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const WS_URL: &str = "wss://advanced-trade-ws.coinbase.com";
const PRICE_CHANNEL_BUFFER: usize = 1000;

/// Coinbase ticker event
#[derive(Debug, Deserialize)]
struct CoinbaseTicker {
    product_id: String,
    price: String,
}

/// Coinbase event wrapper
#[derive(Debug, Deserialize)]
struct CoinbaseEvent {
    tickers: Vec<CoinbaseTicker>,
}

/// Coinbase WebSocket message
#[derive(Debug, Deserialize)]
struct CoinbaseMessage {
    channel: String,
    #[serde(rename = "timestamp")]
    timestamp_str: String,
    events: Vec<CoinbaseEvent>,
}

#[derive(Debug, Clone)]
pub struct TickerUpdate {
    pub price: Decimal,
    pub timestamp_ms: i64,
}

pub struct CoinbaseClient {
    product_id: String,
    price_tx: broadcast::Sender<TickerUpdate>,
}

impl CoinbaseClient {
    pub fn new(product_id: &str) -> Self {
        let (price_tx, _) = broadcast::channel(PRICE_CHANNEL_BUFFER);
        Self {
            product_id: product_id.to_string(),
            price_tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TickerUpdate> {
        self.price_tx.subscribe()
    }

    pub async fn start_ticker_ws(self: Arc<Self>) -> Result<()> {
        tracing::debug!("[WS] connecting {}", WS_URL);
        let mut backoff_secs: u64 = 1;
        const MAX_BACKOFF_SECS: u64 = 60;

        loop {
            match self.ws_loop().await {
                Ok(_) => {
                    tracing::warn!("[WS] disconnected, reconnecting...");
                    backoff_secs = 1;
                }
                Err(e) => {
                    tracing::error!("[WS] error: {}, reconnecting in {}s...", e, backoff_secs);
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
                    tracing::warn!("[WS] error: {}", e);
                    break;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn handle_message(&self, text: &str) {
        if let Ok(msg) = serde_json::from_str::<CoinbaseMessage>(text) {
            if msg.channel != "ticker" {
                return;
            }

            let timestamp_ms = chrono::DateTime::parse_from_rfc3339(&msg.timestamp_str)
                .map(|dt| dt.timestamp_millis())
                .unwrap_or_else(|_| chrono::Utc::now().timestamp_millis());

            for event in msg.events {
                for ticker in event.tickers {
                    if ticker.product_id != self.product_id {
                        continue;
                    }
                    if let Ok(price) = Decimal::from_str_exact(&ticker.price) {
                        if self
                            .price_tx
                            .send(TickerUpdate {
                                price,
                                timestamp_ms,
                            })
                            .is_err()
                        {
                            tracing::debug!("[WS] no price receivers");
                        }
                    }
                }
            }
        }
    }
}
