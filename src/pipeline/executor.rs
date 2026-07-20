//! Stage 4: Order Executor
//! Places value-capped FAK orders in paper or live mode.

use crate::config::TradingMode;
use crate::data::polymarket::AuthenticatedPolyClient;
use crate::pipeline::decider::Decision;
use crate::pipeline::decider::Direction;
use anyhow::Result;
use rust_decimal::Decimal;

#[derive(Debug, Clone)]
pub struct OrderResult {
    pub order_id: String,
    pub direction: Direction,
    pub size_usdc: Decimal,
    pub entry_price: Decimal,
    pub filled_shares: Decimal,
    pub cost: Decimal,
    pub settlement_time_ms: i64,
    pub entry_btc_price: Decimal,
}

pub struct Executor {
    mode: TradingMode,
    auth_client: Option<AuthenticatedPolyClient>,
}

pub struct ExecuteContext<'a> {
    pub decision: &'a Decision,
    pub token_yes: &'a str,
    pub token_no: &'a str,
    pub settlement_time_ms: i64,
    pub btc_price: Decimal,
}

impl Executor {
    pub fn new(mode: TradingMode, auth_client: Option<AuthenticatedPolyClient>) -> Self {
        Self { mode, auth_client }
    }

    pub async fn execute(&self, ctx: &ExecuteContext<'_>) -> Option<OrderResult> {
        match ctx.decision {
            Decision::Pass(_) => None,
            Decision::Trade {
                direction,
                size_usdc,
                entry_price,
                order_limit_price,
                ..
            } => {
                let token_id = match direction {
                    Direction::Up => ctx.token_yes,
                    Direction::Down => ctx.token_no,
                };
                if *entry_price <= Decimal::new(1, 2)
                    || *entry_price >= Decimal::new(99, 2)
                    || *order_limit_price < *entry_price
                {
                    tracing::warn!("[EXEC] Invalid value-capped quote {:.3}", entry_price);
                    return None;
                }

                // Round down so the submitted limit never exceeds the value cap.
                let price = order_limit_price
                    .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::ToNegativeInfinity);
                let mut filled_shares = match Self::compute_filled_shares(*size_usdc, *entry_price)
                {
                    Some(shares) => shares,
                    None => return None,
                };
                let mut cost = *size_usdc;

                let order_id = if self.mode.is_live() {
                    match self.place_live_order(token_id, price, *size_usdc).await {
                        Ok((id, actual_shares, actual_cost)) => {
                            filled_shares = actual_shares;
                            cost = actual_cost;
                            let actual_price = cost / filled_shares;
                            tracing::debug!(
                                "[EXEC] filled id={} shares={} cost={:.2} avg={:.3}",
                                id.get(..8).unwrap_or(&id),
                                filled_shares,
                                cost,
                                actual_price,
                            );
                            id
                        }
                        Err(e) => {
                            let msg = format!("{:#}", e);
                            if msg.contains("not matched")
                                || msg.contains("FAK")
                                || msg.contains("no fill")
                                || msg.contains("fully filled")
                            {
                                tracing::warn!(
                                    "[EXEC] FAK rejected (no liquidity at {:.3})",
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

                let entry_price = cost / filled_shares;
                Some(OrderResult {
                    order_id,
                    direction: *direction,
                    size_usdc: cost,
                    entry_price,
                    filled_shares,
                    cost,
                    settlement_time_ms: ctx.settlement_time_ms,
                    entry_btc_price: ctx.btc_price,
                })
            }
        }
    }

    fn compute_filled_shares(size_usdc: Decimal, price: Decimal) -> Option<Decimal> {
        // Match the SDK's fixed-USDC market-order precision: tick precision
        // (normally 2) plus the 2-decimal lot-size precision.
        if size_usdc < Decimal::ONE {
            tracing::warn!("[EXEC] Order amount {} is below $1 minimum", size_usdc);
            return None;
        }
        let shares = (size_usdc / price).trunc_with_scale(4);
        if shares > Decimal::ZERO {
            Some(shares)
        } else {
            tracing::warn!(
                "[EXEC] Computed 0 shares for size={} price={}",
                size_usdc,
                price
            );
            None
        }
    }

    async fn place_live_order(
        &self,
        token_id: &str,
        price: Decimal,
        amount_usdc: Decimal,
    ) -> Result<(String, Decimal, Decimal)> {
        let client = self
            .auth_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No authenticated client — run with PRIVATE_KEY set"))?;
        client
            .place_order(token_id, "BUY", price, amount_usdc)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::test_helpers::d;

    #[tokio::test]
    async fn test_execute_tracks_filled_shares_and_effective_cost() {
        let executor = Executor::new(TradingMode::Paper, None);
        let decision = Decision::Trade {
            direction: Direction::Up,
            size_usdc: d("5.00"),
            edge: d("0.20"),
            payoff_ratio: d("3.98"),
            model_probability: d("0.80"),
            entry_price: d("0.20"),
            order_limit_price: d("0.21"),
        };

        let result = executor
            .execute(&ExecuteContext {
                decision: &decision,
                token_yes: "yes",
                token_no: "no",
                settlement_time_ms: 123,
                btc_price: d("70000"),
            })
            .await
            .expect("expected paper order");

        assert_eq!(result.filled_shares, d("25"));
        assert_eq!(result.cost, d("5.00"));
    }

    #[tokio::test]
    async fn test_returns_none_when_limit_is_below_entry_quote() {
        let executor = Executor::new(TradingMode::Paper, None);
        let decision = Decision::Trade {
            direction: Direction::Up,
            size_usdc: d("5.00"),
            edge: d("0.20"),
            payoff_ratio: d("3.98"),
            model_probability: d("0.80"),
            entry_price: d("0.21"),
            order_limit_price: d("0.20"),
        };

        let result = executor
            .execute(&ExecuteContext {
                decision: &decision,
                token_yes: "yes",
                token_no: "no",
                settlement_time_ms: 123,
                btc_price: d("70000"),
            })
            .await;
        assert!(result.is_none());
    }

    #[test]
    fn test_compute_filled_shares_returns_none_for_tiny_orders() {
        assert!(Executor::compute_filled_shares(d("0.50"), d("0.60")).is_none());
    }

    #[test]
    fn test_compute_filled_shares_returns_some_for_valid_orders() {
        assert_eq!(
            Executor::compute_filled_shares(d("5.00"), d("0.20")),
            Some(d("25"))
        );
    }
}
