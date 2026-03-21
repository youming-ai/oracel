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
            } => {
                let (token_id, mid_price) = match direction {
                    Direction::Up => (ctx.token_yes, ctx.poly_yes?),
                    Direction::Down => (ctx.token_no, ctx.poly_no?),
                };
                // Use best ask from orderbook when available, but reject if
                // it deviates too far from mid price (indicates thin liquidity).
                // Max allowed: 2× mid price — beyond that, the fill price
                // destroys the strategy's edge.
                let price = match ctx.best_ask {
                    Some(ask) if ask <= mid_price * Decimal::TWO => {
                        if ask != mid_price {
                            tracing::info!(
                                "[EXEC] Using best ask {:.3} (mid was {:.3})",
                                ask,
                                mid_price
                            );
                        }
                        ask
                    }
                    Some(ask) => {
                        tracing::warn!(
                            "[EXEC] Best ask {:.3} too far from mid {:.3} (>{:.0}×), skipping",
                            ask,
                            mid_price,
                            ask / mid_price,
                        );
                        return None;
                    }
                    None => {
                        // Paper mode: simulate spread by adding 1 cent to mid price.
                        // Real orderbooks rarely fill at mid — this makes paper
                        // results more representative of live execution.
                        if self.mode.is_paper() {
                            mid_price + Decimal::new(1, 2)
                        } else {
                            mid_price
                        }
                    }
                };

                if price <= Decimal::new(1, 2) || price >= Decimal::new(99, 2) {
                    tracing::warn!("[EXEC] Extreme price {:.3}, skipping", price);
                    return None;
                }

                // Real-edge check: the decider computed edge using mid price,
                // but the actual fill price may be worse.  Recompute edge at
                // fill price and reject if it drops below half the original.
                let real_edge = Decimal::new(50, 2) - price; // fair_value - fill_price
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
            })
            .await
            .expect("expected paper order");

        // Paper mode adds 1¢ spread: mid 0.201 → simulated 0.211
        assert_eq!(result.filled_shares, d("23"));
        assert_eq!(result.cost, d("4.853"));
        assert!(result.cost <= d("5.00"));
    }

    #[tokio::test]
    async fn test_returns_none_when_price_missing() {
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
                poly_yes: None,
                poly_no: Some(d("0.80")),
                best_ask: None,
                settlement_time_ms: 123,
                btc_price: 70000.0,
            })
            .await;

        assert!(result.is_none());
    }

}
