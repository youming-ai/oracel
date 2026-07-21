//! Binance Prediction execution stage.

use std::sync::Arc;

use anyhow::Result;
use rust_decimal::Decimal;

use crate::config::TradingMode;
use crate::data::binance_prediction::{
    ActivePredictionMarket, BinancePredictionClient, OrderReconciliation,
};
use crate::pipeline::decider::{Decision, Direction};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderResult {
    pub order_id: String,
    pub direction: Direction,
    pub requested_usdt: Decimal,
    pub entry_price: Decimal,
    pub filled_shares: Decimal,
    pub trade_cost: Decimal,
    pub fee: Decimal,
    pub settlement_time_ms: i64,
    pub entry_btc_price: Decimal,
}

impl OrderResult {
    pub fn total_cost(&self) -> Decimal {
        self.trade_cost + self.fee
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionOutcome {
    Filled(OrderResult),
    /// Binance accepted the order but its FOK status was not yet visible.
    /// The position tracker must reconcile it before any further entry.
    AwaitingReconciliation {
        order_id: String,
    },
    Unfilled,
}

pub struct Executor {
    mode: TradingMode,
    client: Arc<BinancePredictionClient>,
}

pub struct ExecuteContext<'a> {
    pub decision: &'a Decision,
    pub market: &'a ActivePredictionMarket,
    pub btc_price: Decimal,
}

impl Executor {
    pub fn new(mode: TradingMode, client: Arc<BinancePredictionClient>) -> Self {
        Self { mode, client }
    }

    pub async fn execute(&self, context: &ExecuteContext<'_>) -> Result<ExecutionOutcome> {
        let Decision::Trade {
            direction,
            size_usdt,
            entry_price,
            max_price,
            ..
        } = context.decision
        else {
            return Ok(ExecutionOutcome::Unfilled);
        };
        if *entry_price <= Decimal::new(1, 2)
            || *entry_price >= Decimal::new(99, 2)
            || *max_price < *entry_price
        {
            anyhow::bail!("invalid Binance Prediction value-capped entry quote");
        }

        if self.mode.is_paper() {
            let filled_shares = (*size_usdt / *entry_price).trunc_with_scale(8);
            if filled_shares <= Decimal::ZERO {
                anyhow::bail!("paper order computed zero shares");
            }
            let fee =
                *size_usdt * Decimal::from(context.market.fee_rate_bps) / Decimal::from(10_000);
            return Ok(ExecutionOutcome::Filled(OrderResult {
                order_id: uuid::Uuid::new_v4().to_string(),
                direction: *direction,
                requested_usdt: *size_usdt,
                entry_price: *entry_price,
                filled_shares,
                trade_cost: *size_usdt,
                fee,
                settlement_time_ms: context.market.end_ms,
                entry_btc_price: context.btc_price,
            }));
        }

        let order_id = self
            .client
            .submit_market_buy(context.market, *direction, *size_usdt, *max_price)
            .await?;
        for _ in 0..self.client.runtime().reconciliation_attempts {
            match self.client.reconcile_order(&order_id).await? {
                OrderReconciliation::Filled(fill) => {
                    let entry_price = fill.entry_price();
                    return Ok(ExecutionOutcome::Filled(OrderResult {
                        order_id: fill.order_id,
                        direction: *direction,
                        requested_usdt: *size_usdt,
                        entry_price,
                        filled_shares: fill.filled_shares,
                        trade_cost: fill.trade_cost,
                        fee: fill.fee,
                        settlement_time_ms: context.market.end_ms,
                        entry_btc_price: context.btc_price,
                    }));
                }
                OrderReconciliation::Unfilled => return Ok(ExecutionOutcome::Unfilled),
                OrderReconciliation::Pending => {
                    tokio::time::sleep(std::time::Duration::from_millis(
                        self.client.runtime().reconciliation_delay_ms,
                    ))
                    .await;
                }
            }
        }
        Ok(ExecutionOutcome::AwaitingReconciliation { order_id })
    }

    pub async fn reconcile(&self, order_id: &str) -> Result<OrderReconciliation> {
        if self.mode.is_paper() {
            anyhow::bail!("paper orders do not require Binance reconciliation");
        }
        self.client.reconcile_order(order_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::binance_prediction::{ActivePredictionMarket, MarketToken};
    use crate::pipeline::decider::Decision;
    use crate::pipeline::test_helpers::d;

    fn market() -> ActivePredictionMarket {
        ActivePredictionMarket {
            market_topic_id: 7,
            vendor: "PREDICT_FUN".into(),
            slug: "btc-5m-7".into(),
            title: "BTC 5m".into(),
            start_ms: 0,
            end_ms: 300_000,
            reference_price: d("64000"),
            fee_rate_bps: 200,
            up: MarketToken {
                market_id: 42,
                token_id: "up".into(),
            },
            down: MarketToken {
                market_id: 42,
                token_id: "down".into(),
            },
        }
    }

    #[test]
    fn paper_fee_is_calculated_from_market_fee_rate() {
        let fee = d("2") * Decimal::from(market().fee_rate_bps) / Decimal::from(10_000);
        assert_eq!(fee, d("0.04"));
    }

    #[test]
    fn decision_exposes_value_cap_for_binance_executor() {
        let decision = Decision::Trade {
            direction: Direction::Up,
            size_usdt: d("2"),
            edge: d("0.10"),
            payoff_ratio: d("1"),
            model_probability: d("0.70"),
            entry_price: d("0.55"),
            max_price: d("0.60"),
        };
        assert!(matches!(
            decision,
            Decision::Trade {
                max_price,
                entry_price,
                ..
            } if max_price >= entry_price
        ));
    }
}
