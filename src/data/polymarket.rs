//! Polymarket CLOB data fetcher
//!
//! Provides REST API for price fetching and order placement.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::types::Side;

// ─── CLOB API Types ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClobPrice {
    pub price: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketData {
    pub condition_id: String,
    pub token_id_yes: String,
    pub token_id_no: String,
    pub question: Option<String>,
    pub end_date: Option<String>,
}

// ─── Polymarket Client ───

const CLOB_REST_URL: &str = "https://clob.polymarket.com";

pub struct PolymarketClient {
    clob_base: String,
}

impl PolymarketClient {
    pub fn new(_token_id_yes: &str, _token_id_no: &str) -> Self {
        Self {
            clob_base: CLOB_REST_URL.to_string(),
        }
    }

    /// Fetch mid price for a token
    pub async fn fetch_mid_price(&self, token_id: &str) -> Result<f64> {
        let url = format!("{}/price?token_id={}&side=buy", self.clob_base, token_id);
        let client = reqwest::Client::new();
        let resp = client
            .get(&url)
            .send()
            .await
            .context("CLOB price request failed")?;
        
        let status = resp.status();
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        
        if !status.is_success() {
            let err = body.get("error").and_then(|e| e.as_str()).unwrap_or("unknown");
            return Err(anyhow::anyhow!("CLOB price error ({}): {}", status, err));
        }
        
        let price = body.get("price")
            .and_then(|p| p.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        
        Ok(price)
    }

    /// Place a limit order on Polymarket CLOB (requires EIP-712 signing)
    pub async fn place_order(
        &self,
        private_key: &str,
        token_id: &str,
        side: &str, // "BUY" or "SELL"
        price: f64,
        size: f64,
    ) -> Result<String> {
        use crate::signing;

        let key_hex = private_key.strip_prefix("0x").unwrap_or(private_key);
        let wallet: ethers::signers::LocalWallet = key_hex.parse()
            .context("Invalid private key")?;

        let order_side = if side == "BUY" {
            signing::OrderSide::Buy
        } else {
            signing::OrderSide::Sell
        };

        let signed_order = signing::create_signed_order(
            &wallet,
            token_id,
            order_side,
            price,
            size,
            0,
            false,
        ).await?;

        let url = format!("{}/order", self.clob_base);
        let client = reqwest::Client::new();

        let payload = serde_json::json!({
            "order": {
                "salt": signed_order.salt,
                "maker": signed_order.maker,
                "signer": signed_order.signer,
                "taker": signed_order.taker,
                "tokenId": signed_order.token_id,
                "makerAmount": signed_order.maker_amount,
                "takerAmount": signed_order.taker_amount,
                "side": order_side.as_u8(),
                "expiration": signed_order.expiration,
                "nonce": signed_order.nonce,
                "feeRateBps": signed_order.fee_rate_bps,
                "signatureType": signed_order.signature_type,
            },
            "signature": signed_order.signature,
            "owner": signed_order.maker,
        });

        let resp = client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .context("CLOB order request failed")?;

        let status = resp.status();
        let body: serde_json::Value = resp.json().await.unwrap_or_default();

        if !status.is_success() {
            return Err(anyhow::anyhow!("Order failed: {} - {}", status, body));
        }

        let order_id = body["orderID"]
            .as_str()
            .or_else(|| body["id"].as_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(order_id)
    }

    /// Get market metadata from Polymarket Gamma API
    pub async fn fetch_market(slug: &str) -> Result<MarketData> {
        let url = format!(
            "https://gamma-api.polymarket.com/markets?slug={}",
            slug
        );
        let client = reqwest::Client::new();
        let resp: serde_json::Value = client
            .get(&url)
            .send()
            .await
            .context("Gamma API request failed")?
            .json()
            .await?;

        let market = resp
            .as_array()
            .and_then(|arr| arr.first())
            .ok_or_else(|| anyhow::anyhow!("No market found for slug: {}", slug))?;

        let condition_id = market["condition_id"].as_str().unwrap_or("").to_string();
        let token_ids: Vec<String> = market["clobTokenIds"]
            .as_str()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        let token_id_yes = token_ids.first().cloned().unwrap_or_default();
        let token_id_no = token_ids.get(1).cloned().unwrap_or_default();

        Ok(MarketData {
            condition_id,
            token_id_yes,
            token_id_no,
            question: market["question"].as_str().map(String::from),
            end_date: market["endDate"].as_str().map(String::from),
        })
    }
}
