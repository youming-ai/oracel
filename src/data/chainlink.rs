//! Polymarket RTDS Chainlink BTC/USD price source.
//!
//! BTC 5-minute markets resolve against this feed, so it is the authoritative
//! source for the window opening price and the current distance from it.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use rust_decimal::{Decimal, MathematicalOps};
use serde_json::Value;
use tokio::sync::RwLock;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const RTDS_URL: &str = "wss://ws-live-data.polymarket.com";
const SYMBOL: &str = "btc/usd";
const RECONNECT_MAX_SECS: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChainlinkTick {
    pub price: Decimal,
    pub timestamp_ms: i64,
}

pub struct ChainlinkSource {
    buffer: RwLock<VecDeque<ChainlinkTick>>,
    max: usize,
}

impl ChainlinkSource {
    pub fn new(max: usize) -> Self {
        Self {
            buffer: RwLock::new(VecDeque::with_capacity(max)),
            max,
        }
    }

    pub async fn latest(&self) -> Option<ChainlinkTick> {
        self.buffer.read().await.back().copied()
    }

    /// Return the first authoritative tick at, or immediately after, a window boundary.
    /// A gap greater than five seconds is rejected rather than inventing an opening price.
    pub async fn opening_price(&self, window_start_ms: i64) -> Option<Decimal> {
        let max_timestamp = window_start_ms.saturating_add(5_000);
        self.buffer
            .read()
            .await
            .iter()
            .find(|tick| tick.timestamp_ms >= window_start_ms && tick.timestamp_ms <= max_timestamp)
            .map(|tick| tick.price)
    }

    /// Estimate one-second realized volatility from authoritative price returns.
    pub async fn realized_sigma_per_second(
        &self,
        lookback_secs: u64,
        min_samples: usize,
    ) -> Option<Decimal> {
        let buffer = self.buffer.read().await;
        let latest = buffer.back()?;
        let cutoff = latest
            .timestamp_ms
            .saturating_sub(i64::try_from(lookback_secs).ok()?.saturating_mul(1_000));
        let ticks: Vec<_> = buffer
            .iter()
            .filter(|tick| tick.timestamp_ms >= cutoff)
            .collect();
        if ticks.len() <= min_samples {
            return None;
        }
        let mut squared_returns = Decimal::ZERO;
        let mut elapsed_ms = 0_i64;
        let mut samples = 0_usize;
        for pair in ticks.windows(2) {
            let interval_ms = pair[1].timestamp_ms.saturating_sub(pair[0].timestamp_ms);
            if pair[0].price <= Decimal::ZERO || interval_ms <= 0 {
                continue;
            }
            let price_return = (pair[1].price - pair[0].price) / pair[0].price;
            squared_returns += price_return * price_return;
            elapsed_ms = elapsed_ms.saturating_add(interval_ms);
            samples += 1;
        }
        if samples < min_samples || elapsed_ms <= 0 {
            return None;
        }
        let elapsed_seconds = Decimal::from(elapsed_ms) / Decimal::from(1_000);
        (squared_returns / elapsed_seconds).sqrt()
    }

    pub async fn start(self: Arc<Self>, shutdown: Arc<AtomicBool>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut backoff_secs = 1;
            while !shutdown.load(Ordering::Acquire) {
                match self.run_ws_loop(&shutdown).await {
                    Ok(()) => backoff_secs = 1,
                    Err(error) => tracing::warn!(
                        "[WS] Chainlink RTDS error: {:#}; reconnecting in {}s",
                        error,
                        backoff_secs
                    ),
                }
                if shutdown.load(Ordering::Acquire) {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                backoff_secs = (backoff_secs * 2).min(RECONNECT_MAX_SECS);
            }
        })
    }

    async fn run_ws_loop(&self, shutdown: &AtomicBool) -> anyhow::Result<()> {
        tracing::debug!("[WS] connecting to Chainlink RTDS {}", RTDS_URL);
        let (stream, _) = tokio::time::timeout(Duration::from_secs(10), connect_async(RTDS_URL))
            .await
            .map_err(|_| anyhow::anyhow!("Chainlink RTDS connect timed out after 10s"))?
            .context("Chainlink RTDS connect failed")?;
        let (mut write, mut read) = stream.split();
        let subscription = serde_json::json!({
            "action": "subscribe",
            "subscriptions": [{
                "topic": "crypto_prices_chainlink",
                "type": "*",
                "filters": r#"{"symbol":"btc/usd"}"#
            }]
        });
        write
            .send(Message::Text(subscription.to_string()))
            .await
            .context("Chainlink RTDS subscription failed")?;

        while !shutdown.load(Ordering::Acquire) {
            let message = match read.next().await {
                Some(message) => message.context("Chainlink RTDS read failed")?,
                None => break,
            };
            match message {
                Message::Text(text) => {
                    for tick in parse_ticks(&text) {
                        self.push(tick).await;
                    }
                }
                Message::Ping(data) => {
                    write
                        .send(Message::Pong(data))
                        .await
                        .context("Chainlink RTDS pong failed")?;
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
        Ok(())
    }

    async fn push(&self, tick: ChainlinkTick) {
        let mut buffer = self.buffer.write().await;
        match buffer.back_mut() {
            Some(last) if tick.timestamp_ms < last.timestamp_ms => return,
            Some(last) if tick.timestamp_ms == last.timestamp_ms => {
                *last = tick;
                return;
            }
            _ => buffer.push_back(tick),
        }
        while buffer.len() > self.max {
            buffer.pop_front();
        }
    }
}

fn decimal_from_value(value: &Value) -> Option<Decimal> {
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => return None,
    };
    Decimal::from_str_exact(&text)
        .or_else(|_| Decimal::from_scientific(&text))
        .ok()
}

fn tick_from_payload(payload: &Value) -> Option<ChainlinkTick> {
    if payload.get("symbol")?.as_str()? != SYMBOL {
        return None;
    }
    let timestamp_ms = payload.get("timestamp")?.as_i64()?;
    let price = decimal_from_value(payload.get("value")?)?;
    (price > Decimal::ZERO).then_some(ChainlinkTick {
        price,
        timestamp_ms,
    })
}

fn parse_ticks(text: &str) -> Vec<ChainlinkTick> {
    let Ok(message) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(payload) = message.get("payload") else {
        return Vec::new();
    };

    if let Some(tick) = tick_from_payload(payload) {
        return vec![tick];
    }

    let symbol_matches = payload.get("symbol").and_then(Value::as_str) == Some(SYMBOL);
    if !symbol_matches {
        return Vec::new();
    }
    payload
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let timestamp_ms = item.get("timestamp")?.as_i64()?;
            let price = decimal_from_value(item.get("value")?)?;
            (price > Decimal::ZERO).then_some(ChainlinkTick {
                price,
                timestamp_ms,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_update_without_binary_float_conversion() {
        let ticks = parse_ticks(
            r#"{"topic":"crypto_prices_chainlink","type":"update","payload":{"symbol":"btc/usd","timestamp":1700000000000,"value":64033.70429892299}}"#,
        );
        assert_eq!(ticks.len(), 1);
        assert_eq!(
            ticks[0].price,
            Decimal::from_str_exact("64033.70429892299").unwrap()
        );
        assert_eq!(ticks[0].timestamp_ms, 1_700_000_000_000);
    }

    #[test]
    fn parses_initial_history_dump() {
        let ticks = parse_ticks(
            r#"{"payload":{"data":[{"timestamp":1700000000000,"value":64000.1},{"timestamp":1700000001000,"value":64001.2}],"symbol":"btc/usd"},"topic":"crypto_prices","type":"subscribe"}"#,
        );
        assert_eq!(ticks.len(), 2);
        assert_eq!(ticks[1].price, Decimal::from_str_exact("64001.2").unwrap());
    }

    #[tokio::test]
    async fn opening_price_requires_tick_near_boundary() {
        let source = ChainlinkSource::new(10);
        source
            .push(ChainlinkTick {
                price: Decimal::from(64_000),
                timestamp_ms: 301_000,
            })
            .await;
        assert_eq!(
            source.opening_price(300_000).await,
            Some(Decimal::from(64_000))
        );
        assert_eq!(source.opening_price(290_000).await, None);
    }

    #[tokio::test]
    async fn realized_volatility_requires_enough_samples() {
        let source = ChainlinkSource::new(10);
        for (second, price) in ["100", "101", "100", "102"].into_iter().enumerate() {
            source
                .push(ChainlinkTick {
                    price: Decimal::from_str_exact(price).unwrap(),
                    timestamp_ms: second as i64 * 1_000,
                })
                .await;
        }
        assert!(source.realized_sigma_per_second(10, 3).await.is_some());
        assert!(source.realized_sigma_per_second(10, 4).await.is_none());
    }
}
