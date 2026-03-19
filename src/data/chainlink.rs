//! Chainlink BTC/USD oracle price on Polygon.
//!
//! Paper mode: free public RPC
//! Live mode: Alchemy (via ALCHEMY_KEY env var)

use crate::config::TradingMode;
use anyhow::{Context, Result};
use std::time::Duration;

const PUBLIC_RPC: &str = "https://polygon-bor-rpc.publicnode.com";
const ALCHEMY_RPC: &str = "https://polygon-mainnet.g.alchemy.com/v2";
const CHAINLINK_BTC_USD: &str = "0xc907E116054Ad103354f2D350FD2514433D57F6f";
const LATEST_ROUND_DATA: &str = "0xfeaf968c";

/// Pick RPC based on mode: live uses Alchemy (ALCHEMY_KEY env), paper uses public.
pub(crate) fn rpc_url(mode: TradingMode) -> String {
    if mode.is_live() {
        if let Ok(key) = std::env::var("ALCHEMY_KEY") {
            return format!("{}/{}", ALCHEMY_RPC, key);
        }
        tracing::warn!("[RPC] ALCHEMY_KEY not set, falling back to public RPC");
    }
    PUBLIC_RPC.to_string()
}

pub(crate) async fn fetch_btc_price(client: &reqwest::Client, rpc: &str) -> Result<f64> {
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_call",
        "params": [{"to": CHAINLINK_BTC_USD, "data": LATEST_ROUND_DATA}, "latest"],
        "id": 1
    });

    let request = client.post(rpc).json(&payload);

    let resp: serde_json::Value = tokio::time::timeout(Duration::from_secs(10), request.send())
        .await
        .map_err(|_| anyhow::anyhow!("Chainlink RPC timed out after 10s"))?
        .context("Chainlink RPC failed")?
        .json()
        .await?;

    let result = resp["result"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No result from Chainlink"))?;

    if result.len() < 130 {
        return Err(anyhow::anyhow!("Chainlink response too short"));
    }

    let answer =
        i128::from_str_radix(&result[66..130], 16).context("Failed to parse Chainlink answer")?;

    Ok(answer as f64 / 1e8)
}
