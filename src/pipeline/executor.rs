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
    /// Best ask price from the order book (live mode only).
    /// When set, FOK orders use this instead of mid price.
    pub best_ask: Option<Decimal>,
    pub settlement_time_ms: i64,
    pub btc_price: f64,
    pub fair_value: Decimal,
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
                edge,
                ..
            } => {
                let (token_id, mid_price) = match direction {
                    Direction::Up => (ctx.token_yes, ctx.poly_yes?),
                    Direction::Down => (ctx.token_no, ctx.poly_no?),
                };
                // Use best ask from orderbook when available,
                // fall back to mid price if orderbook fetch failed
                let price = ctx.best_ask.unwrap_or(mid_price);
                if ctx.best_ask.is_some() && price != mid_price {
                    tracing::info!(
                        "[EXEC] Using best ask {:.3} (mid was {:.3})",
                        price,
                        mid_price
                    );
                }

                if price <= Decimal::new(1, 2) || price >= Decimal::new(99, 2) {
                    tracing::warn!("[EXEC] Extreme price {:.3}, skipping", price);
                    return None;
                }

                // Real-edge check: the decider computed edge using mid price,
                // but the actual fill price may be worse.  Recompute edge at
                // fill price and reject if it drops below half the original.
                let real_edge = ctx.fair_value - price; // fair_value - fill_price
                if real_edge < *edge / Decimal::TWO {
                    tracing::warn!(
                        "[EXEC] Fill price {:.3} erases edge: real={:.0}% vs signal={:.0}%, skipping",
                        price,
                        real_edge * Decimal::ONE_HUNDRED,
                        *edge * Decimal::ONE_HUNDRED,
                    );
                    return None;
                }

                let mut filled_shares = match Self::compute_filled_shares(*size_usdc, price) {
                    Some(shares) => shares,
                    None => return None,
                };
                let mut cost = filled_shares * price;

                // Polymarket requires minimum $1 order amount; bump shares to meet it
                if cost < Decimal::ONE {
                    filled_shares = (Decimal::ONE / price).ceil();
                    cost = filled_shares * price;
                    tracing::info!(
                        "[EXEC] Bumped to {} shares (cost={:.2}) to meet $1 minimum",
                        filled_shares,
                        cost
                    );
                }
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

    fn compute_filled_shares(size_usdc: Decimal, price: Decimal) -> Option<Decimal> {
        // Floor to whole shares so that maker_amount (= price × shares) stays
        // within the CLOB's 2-decimal-place limit for market buy orders.
        // Returns None if resulting shares would be 0 (reject tiny orders).
        let shares = (size_usdc / price).floor();
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
            payoff_ratio: d("3.98"),
        };

        let result = executor
            .execute(&ExecuteContext {
                decision: &decision,
                token_yes: "yes",
                token_no: "no",
                poly_yes: Some(d("0.201")),
                poly_no: Some(d("0.799")),
                best_ask: None,
                settlement_time_ms: 123,
                btc_price: 70000.0,
                fair_value: d("0.50"),
            })
            .await
            .expect("expected paper order");

        // With no spread: floor(5.00 / 0.201) = 24 shares, cost = 24 × 0.201 = 4.824
        assert_eq!(result.filled_shares, d("24"));
        assert_eq!(result.cost, d("4.824"));
        assert!(result.cost <= d("5.00"));
    }

    #[tokio::test]
    async fn test_returns_none_when_price_missing() {
        let executor = Executor::new(TradingMode::Paper, None);
        let decision = Decision::Trade {
            direction: Direction::Up,
            size_usdc: d("5.00"),
            edge: d("0.20"),
            payoff_ratio: d("3.98"),
        };

        let result = executor
            .execute(&ExecuteContext {
                decision: &decision,
                token_yes: "yes",
                token_no: "no",
                poly_yes: None,
                poly_no: Some(d("0.80")),
                best_ask: None,
                settlement_time_ms: 123,
                btc_price: 70000.0,
                fair_value: d("0.50"),
            })
            .await;

        assert!(result.is_none());
    }

}
