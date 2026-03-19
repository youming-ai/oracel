//! Stage 4: Order Executor
//! Places orders (paper or live).

use crate::config::TradingMode;
use crate::data::polymarket::AuthenticatedPolyClient;
use crate::pipeline::decider::Decision;
use crate::pipeline::signal::Direction;
use anyhow::Result;
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
    mode: TradingMode,
    auth_client: Option<AuthenticatedPolyClient>,
}

pub(crate) struct ExecuteContext<'a> {
    pub decision: &'a Decision,
    pub token_yes: &'a str,
    pub token_no: &'a str,
    pub poly_yes: Option<Decimal>,
    pub poly_no: Option<Decimal>,
    pub settlement_time_ms: i64,
    pub btc_price: f64,
}

impl Executor {
    pub(crate) fn new(mode: TradingMode, auth_client: Option<AuthenticatedPolyClient>) -> Self {
        Self { mode, auth_client }
    }

    pub(crate) async fn execute(&self, ctx: &ExecuteContext<'_>) -> Option<OrderResult> {
        match ctx.decision {
            Decision::Pass(_) => None,
            Decision::Trade {
                direction,
                size_usdc,
                edge: _,
            } => {
                let (token_id, price) = match direction {
                    Direction::Up => (ctx.token_yes, ctx.poly_yes.unwrap_or(Decimal::new(5, 1))),
                    Direction::Down => (ctx.token_no, ctx.poly_no.unwrap_or(Decimal::new(5, 1))),
                };

                if price <= Decimal::new(1, 2) || price >= Decimal::new(99, 2) {
                    tracing::warn!("[EXEC] Extreme price {:.3}, skipping", price);
                    return None;
                }

                let filled_shares = Self::compute_filled_shares(*size_usdc, price);
                let cost = filled_shares * price;
                let order_id = if self.mode.is_live() {
                    match self.place_live_order(token_id, price, filled_shares).await {
                        Ok(id) => {
                            tracing::info!(
                                "[EXEC] filled id={} shares={} cost={:.2}",
                                &id[..8.min(id.len())],
                                filled_shares,
                                cost,
                            );
                            id
                        }
                        Err(e) => {
                            let msg = format!("{:#}", e);
                            if msg.contains("not matched")
                                || msg.contains("FOK")
                                || msg.contains("no fill")
                                || msg.contains("fully filled")
                            {
                                tracing::warn!(
                                    "[EXEC] FOK rejected (no liquidity at {:.3})",
                                    price
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
                    settlement_time_ms: ctx.settlement_time_ms,
                    entry_btc_price: ctx.btc_price,
                })
            }
        }
    }

    fn compute_filled_shares(size_usdc: Decimal, price: Decimal) -> Decimal {
        // Floor to whole shares so that maker_amount (= price × shares) stays
        // within the CLOB's 2-decimal-place limit for market buy orders.
        (size_usdc / price).floor()
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
        client.place_order(token_id, "BUY", price, shares).await
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
        let executor = Executor::new(TradingMode::Paper, None);
        let decision = Decision::Trade {
            direction: Direction::Up,
            size_usdc: d("5.00"),
            edge: d("0.20"),
        };

        let result = executor
            .execute(&ExecuteContext {
                decision: &decision,
                token_yes: "yes",
                token_no: "no",
                poly_yes: Some(d("0.201")),
                poly_no: Some(d("0.799")),
                settlement_time_ms: 123,
                btc_price: 70000.0,
            })
            .await
            .expect("expected paper order");

        assert_eq!(result.filled_shares, d("24"));
        assert_eq!(result.cost, d("4.824"));
        assert!(result.cost <= d("5.00"));
    }
}
