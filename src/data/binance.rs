//! Binance real-time BTC price client (REST API and WebSocket)

use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const WS_URL: &str = "wss://stream.binance.com:9443/ws";
const PRICE_CHANNEL_BUFFER: usize = 1000;

#[derive(Debug, Clone)]
pub(crate) struct TickerUpdate {
    pub price: f64,
    pub timestamp_ms: i64,
}

enum WsLoopError {
    Permanent(String),
    Transient(anyhow::Error),
}

pub(crate) struct BinanceClient {
    symbol: String,
    price_tx: broadcast::Sender<TickerUpdate>,
}

impl BinanceClient {
    pub(crate) fn new(symbol: &str) -> Self {
        let (price_tx, _) = broadcast::channel(PRICE_CHANNEL_BUFFER);
        Self {
            symbol: symbol.to_string(),
            price_tx,
        }
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<TickerUpdate> {
        self.price_tx.subscribe()
    }

    /// Start WebSocket connection for real-time price updates
    pub(crate) async fn start_ticker_ws(self: Arc<Self>) -> anyhow::Result<()> {
        let stream_name = format!("{}@ticker", self.symbol.to_lowercase());
        let ws_url = format!("{}/{}", WS_URL, stream_name);

        tracing::info!("[WS] connecting to Binance {}", ws_url);
        let mut backoff_secs: u64 = 1;
        const MAX_BACKOFF_SECS: u64 = 60;

        loop {
            match self.ws_loop(&ws_url).await {
                Ok(()) => {
                    tracing::warn!("[WS] Binance WS disconnected, reconnecting...");
                    backoff_secs = 1;
                }
                Err(WsLoopError::Permanent(message)) => {
                    tracing::error!("[WS] Binance WS permanent error: {}", message);
                    anyhow::bail!(message);
                }
                Err(WsLoopError::Transient(err)) => {
                    tracing::error!(
                        "[WS] Binance WS error: {}, reconnecting in {}s...",
                        err,
                        backoff_secs
                    );
                }
            }

            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
            backoff_secs = (backoff_secs * 2).min(MAX_BACKOFF_SECS);
        }
    }

    async fn ws_loop(&self, ws_url: &str) -> std::result::Result<(), WsLoopError> {
        let (ws, _) = tokio::time::timeout(Duration::from_secs(10), connect_async(ws_url))
            .await
            .map_err(|_| WsLoopError::Transient(anyhow::anyhow!("WS connect timed out after 10s")))?
            .context("WS connect failed")
            .map_err(WsLoopError::Transient)?;
        let (mut write, mut read) = ws.split();

        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => self.handle_message(&text)?,
                Ok(Message::Ping(data)) => {
                    if let Err(e) = write.send(Message::Pong(data)).await {
                        tracing::debug!("[WS] pong send failed: {}", e);
                    }
                }
                Ok(Message::Close(_)) => break,
                Err(e) => {
                    tracing::warn!("[WS] error: {}", e);
                    return Err(WsLoopError::Transient(anyhow::anyhow!(e)));
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn handle_message(&self, text: &str) -> std::result::Result<(), WsLoopError> {
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(text) {
            if data.get("code").and_then(|v| v.as_i64()) == Some(-1121) {
                let message = data
                    .get("msg")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Invalid symbol");
                return Err(WsLoopError::Permanent(format!(
                    "invalid Binance symbol '{}': {}",
                    self.symbol, message
                )));
            }

            if let Some(price) = data
                .get("c")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok())
            {
                let timestamp_ms = data
                    .get("E")
                    .and_then(|v| v.as_i64())
                    .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());

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

        Ok(())
    }
}
