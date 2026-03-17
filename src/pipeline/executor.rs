//! Stage 4: Order Executor
//! Places orders (paper or live).

use anyhow::Result;
use chrono::Utc;
use crate::pipeline::decider::Decision;
use crate::pipeline::signal::Direction;
use crate::data::polymarket::PolymarketClient;

#[derive(Debug, Clone)]
pub struct OrderResult {
    pub order_id: String,
    pub direction: Direction,
    pub size_usdc: f64,
    pub entry_price: f64,
    pub cost: f64,
    pub token_id: String,
    pub settlement_time_ms: i64,
    pub entry_btc_price: f64,
}

pub struct Executor {
    mode: String,
    private_key: String,
    polymarket: PolymarketClient,
}

impl Executor {
    pub fn new(mode: String, private_key: String, polymarket: PolymarketClient) -> Self {
        Self { mode, private_key, polymarket }
    }

    pub async fn execute(
        &self,
        decision: &Decision,
        token_yes: &str,
        token_no: &str,
        poly_yes: Option<f64>,
        poly_no: Option<f64>,
        settlement_time_ms: i64,
        btc_price: f64,
    ) -> Option<OrderResult> {
        match decision {
            Decision::Pass(_) => None,
            Decision::Trade { direction, size_usdc, edge: _ } => {
                let (token_id, price) = match direction {
                    Direction::Up => (token_yes, poly_yes.unwrap_or(0.5)),
                    Direction::Down => (token_no, poly_no.unwrap_or(0.5)),
                };

                if price <= 0.01 || price >= 0.99 {
                    tracing::warn!("[EXEC] Extreme price {:.3}, skipping", price);
                    return None;
                }

                let cost = size_usdc * price;
                let order_id = uuid::Uuid::new_v4().to_string();

                if self.mode == "paper" {
                    tracing::info!(
                        "[PAPER] {} {} ${:.2} @ {:.3} (cost: ${:.2}) BTC=${:.0}",
                        &order_id[..8], direction.as_str(), size_usdc, price, cost, btc_price
                    );
                } else {
                    // Live mode: place actual order via Polymarket CLOB
                    match self.place_live_order(token_id, price, *size_usdc).await {
                        Ok(real_order_id) => {
                            tracing::info!(
                                "[LIVE] {} {} ${:.2} @ {:.3} (cost: ${:.2}) BTC=${:.0} | order_id={}",
                                &order_id[..8], direction.as_str(), size_usdc, price, cost, btc_price,
                                &real_order_id[..16.min(real_order_id.len())]
                            );
                        }
                        Err(e) => {
                            tracing::error!("[LIVE] Order failed: {}", e);
                            return None;
                        }
                    }
                }

                Some(OrderResult {
                    order_id,
                    direction: *direction,
                    size_usdc: *size_usdc,
                    entry_price: price,
                    cost,
                    token_id: token_id.to_string(),
                    settlement_time_ms,
                    entry_btc_price: btc_price,
                })
            }
        }
    }

    async fn place_live_order(&self, token_id: &str, price: f64, size_usdc: f64) -> Result<String> {
        use crate::signing;
        
        // Parse private key
        let key_hex = self.private_key.strip_prefix("0x").unwrap_or(&self.private_key);
        let wallet: ethers::signers::LocalWallet = key_hex.parse()
            .map_err(|e| anyhow::anyhow!("Invalid private key: {}", e))?;
        
        // Size in shares = USDC / price
        let shares = size_usdc / price;
        
        let signed_order = signing::create_signed_order(
            &wallet,
            token_id,
            signing::OrderSide::Buy,
            price,
            shares,
            0,      // GTC expiration
            false,  // not neg-risk
        ).await?;
        
        // Submit to CLOB
        let url = format!("https://clob.polymarket.com/order");
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
                "side": 0u8,  // BUY
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
            .map_err(|e| anyhow::anyhow!("CLOB request failed: {}", e))?;
        
        let status = resp.status();
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        
        if !status.is_success() {
            return Err(anyhow::anyhow!("Order rejected ({}): {}", status, body));
        }
        
        let order_id = body["orderID"]
            .as_str()
            .or_else(|| body["id"].as_str())
            .unwrap_or("unknown")
            .to_string();
        
        Ok(order_id)
    }
}
