//! Stage 4: Order Executor
//! Places orders (paper or live).

use anyhow::Result;
use crate::pipeline::decider::Decision;
use crate::pipeline::signal::Direction;
use crate::data::polymarket::AuthenticatedPolyClient;

#[derive(Debug, Clone)]
pub struct OrderResult {
    pub order_id: String,
    pub direction: Direction,
    pub size_usdc: f64,
    pub entry_price: f64,
    pub cost: f64,
    pub settlement_time_ms: i64,
    pub entry_btc_price: f64,
}

pub struct Executor {
    mode: String,
    auth_client: Option<AuthenticatedPolyClient>,
}

impl Executor {
    pub fn new(mode: String, auth_client: Option<AuthenticatedPolyClient>) -> Self {
        Self { mode, auth_client }
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

                let cost = *size_usdc;
                let order_id = if self.mode != "paper" {
                    match self.place_live_order(token_id, price, *size_usdc).await {
                        Ok(id) => id,
                        Err(e) => {
                            let msg = e.to_string();
                            if msg.contains("not matched") || msg.contains("FOK") || msg.contains("no fill") {
                                tracing::warn!("[EXEC] FOK rejected (no liquidity at {:.3}): {}", price, msg);
                            } else {
                                tracing::error!("[EXEC] order failed: {}", msg);
                            }
                            return None;
                        }
                    }
                } else {
                    uuid::Uuid::new_v4().to_string()
                };

                Some(OrderResult {
                    order_id,
                    direction: *direction,
                    size_usdc: *size_usdc,
                    entry_price: price,
                    cost,
                    settlement_time_ms,
                    entry_btc_price: btc_price,
                })
            }
        }
    }

    async fn place_live_order(&self, token_id: &str, price: f64, size_usdc: f64) -> Result<String> {
        let client = self.auth_client.as_ref()
            .ok_or_else(|| anyhow::anyhow!("No authenticated client — run with PRIVATE_KEY set"))?;
        // Truncate to 2 decimal places (LOT_SIZE_SCALE) — floor to never over-order
        let shares = ((size_usdc / price) * 100.0).floor() / 100.0;
        client.place_order(token_id, "BUY", price, shares).await
    }
}
