//! Stage 4: Order Executor
//! Places orders (paper or live).

use crate::data::polymarket::AuthenticatedPolyClient;
use crate::pipeline::decider::Decision;
use crate::pipeline::signal::Direction;
use anyhow::Result;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

#[derive(Debug, Clone)]
pub(crate) struct OrderResult {
    pub order_id: String,
    pub direction: Direction,
    pub size_usdc: Decimal,
    pub entry_price: Decimal,
    pub filled_shares: Decimal,
    pub cost: Decimal,
    pub settlement_time_ms: i64,
    pub entry_btc_price: f64,
}

pub(crate) struct Executor {
    mode: String,
    auth_client: Option<AuthenticatedPolyClient>,
}

impl Executor {
    pub(crate) fn new(mode: String, auth_client: Option<AuthenticatedPolyClient>) -> Self {
        Self { mode, auth_client }
    }

    pub(crate) async fn execute(
        &self,
        decision: &Decision,
        token_yes: &str,
        token_no: &str,
        poly_yes: Option<Decimal>,
        poly_no: Option<Decimal>,
        settlement_time_ms: i64,
        btc_price: f64,
    ) -> Option<OrderResult> {
        match decision {
            Decision::Pass(_) => None,
            Decision::Trade {
                direction,
                size_usdc,
                edge: _,
            } => {
                let (token_id, price) = match direction {
                    Direction::Up => (token_yes, poly_yes.unwrap_or(Decimal::new(5, 1))),
                    Direction::Down => (token_no, poly_no.unwrap_or(Decimal::new(5, 1))),
                };

                if price <= Decimal::new(1, 2) || price >= Decimal::new(99, 2) {
                    tracing::warn!("[EXEC] Extreme price {:.3}, skipping", price);
                    return None;
                }

                let filled_shares = Self::compute_filled_shares(*size_usdc, price);
                let cost = filled_shares * price;
                let order_id = if self.mode != "paper" {
                    match self.place_live_order(token_id, price, filled_shares).await {
                        Ok(id) => id,
                        Err(e) => {
                            let msg = e.to_string();
                            if msg.contains("not matched")
                                || msg.contains("FOK")
                                || msg.contains("no fill")
                            {
                                tracing::warn!(
                                    "[EXEC] FOK rejected (no liquidity at {:.3}): {}",
                                    price,
                                    msg
                                );
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
                    filled_shares,
                    cost,
                    settlement_time_ms,
                    entry_btc_price: btc_price,
                })
            }
        }
    }

    fn compute_filled_shares(size_usdc: Decimal, price: Decimal) -> Decimal {
        ((size_usdc / price) * Decimal::new(100, 0)).floor() / Decimal::new(100, 0)
    }

    async fn place_live_order(
        &self,
        token_id: &str,
        price: Decimal,
        shares: Decimal,
    ) -> Result<String> {
        let client = self
            .auth_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No authenticated client — run with PRIVATE_KEY set"))?;
        let price_f64 = price
            .to_f64()
            .ok_or_else(|| anyhow::anyhow!("Failed to convert decimal price for order"))?;
        let shares_f64 = shares
            .to_f64()
            .ok_or_else(|| anyhow::anyhow!("Failed to convert decimal shares for order"))?;
        client
            .place_order(token_id, "BUY", price_f64, shares_f64)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(value: &str) -> rust_decimal::Decimal {
        rust_decimal::Decimal::from_str_exact(value).expect("valid decimal")
    }

    #[tokio::test]
    async fn test_execute_tracks_filled_shares_and_effective_cost() {
        let executor = Executor::new("paper".to_string(), None);
        let decision = Decision::Trade {
            direction: Direction::Up,
            size_usdc: d("5.00"),
            edge: d("0.20"),
        };

        let result = executor
            .execute(
                &decision,
                "yes",
                "no",
                Some(d("0.201")),
                Some(d("0.799")),
                123,
                70000.0,
            )
            .await
            .expect("expected paper order");

        assert_eq!(result.filled_shares, d("24.87"));
        assert_eq!(result.cost, d("4.99887"));
        assert!(result.cost <= d("5.00"));
    }
}
