//! Polymarket CLOB REST client — price fetching and order placement.

use anyhow::{Context, Result};

const CLOB_REST_URL: &str = "https://clob.polymarket.com";

pub struct PolymarketClient {
    clob_base: String,
    client: reqwest::Client,
}

impl PolymarketClient {
    pub fn new() -> Self {
        Self {
            clob_base: CLOB_REST_URL.to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub async fn fetch_mid_price(&self, token_id: &str) -> Result<f64> {
        let url = format!("{}/price?token_id={}&side=buy", self.clob_base, token_id);
        let resp = self.client.get(&url).send().await.context("CLOB price request failed")?;
        let status = resp.status();
        let body: serde_json::Value = resp.json().await.unwrap_or_default();

        if !status.is_success() {
            let err = body.get("error").and_then(|e| e.as_str()).unwrap_or("unknown");
            return Err(anyhow::anyhow!("CLOB price error ({}): {}", status, err));
        }

        Ok(body.get("price")
            .and_then(|p| p.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0))
    }

    pub async fn place_order(
        &self,
        private_key: &str,
        token_id: &str,
        side: &str,
        price: f64,
        size: f64,
    ) -> Result<String> {
        use crate::signing;

        let key_hex = private_key.strip_prefix("0x").unwrap_or(private_key);
        let wallet: ethers::signers::LocalWallet = key_hex.parse().context("Invalid private key")?;

        let order_side = if side == "BUY" { signing::OrderSide::Buy } else { signing::OrderSide::Sell };

        let signed_order = signing::create_signed_order(
            &wallet, token_id, order_side, price, size, 0, false,
        ).await?;

        let url = format!("{}/order", self.clob_base);
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

        let resp = self.client.post(&url).json(&payload).send().await.context("CLOB order request failed")?;
        let status = resp.status();
        let body: serde_json::Value = resp.json().await.unwrap_or_default();

        if !status.is_success() {
            return Err(anyhow::anyhow!("Order failed: {} - {}", status, body));
        }

        Ok(body["orderID"].as_str().or_else(|| body["id"].as_str()).unwrap_or("unknown").to_string())
    }
}
