//! Binance Prediction position tracking, reconciliation, and settlement state.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::Context;
use chrono::Utc;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::data::binance_prediction::ActivePredictionMarket;
use crate::pipeline::decider::Direction;
use crate::pipeline::executor::OrderResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionState {
    AwaitingReconciliation,
    Open,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPosition {
    pub execution_state: ExecutionState,
    pub market_topic_id: i64,
    pub market_id: i64,
    pub token_id: String,
    pub market_slug: String,
    pub direction: Direction,
    pub order_id: String,
    pub requested_usdt: Decimal,
    pub entry_price: Decimal,
    pub filled_shares: Decimal,
    pub trade_cost: Decimal,
    pub fee: Decimal,
    pub settlement_time_ms: i64,
    pub entry_btc_price: Decimal,
}

impl PendingPosition {
    pub fn from_filled(market: &ActivePredictionMarket, order: OrderResult) -> Self {
        let token = market.token(order.direction);
        Self {
            execution_state: ExecutionState::Open,
            market_topic_id: market.market_topic_id,
            market_id: token.market_id,
            token_id: token.token_id.clone(),
            market_slug: market.slug.clone(),
            direction: order.direction,
            order_id: order.order_id,
            requested_usdt: order.requested_usdt,
            entry_price: order.entry_price,
            filled_shares: order.filled_shares,
            trade_cost: order.trade_cost,
            fee: order.fee,
            settlement_time_ms: order.settlement_time_ms,
            entry_btc_price: order.entry_btc_price,
        }
    }

    pub fn awaiting_reconciliation(
        market: &ActivePredictionMarket,
        direction: Direction,
        requested_usdt: Decimal,
        order_id: String,
        entry_btc_price: Decimal,
    ) -> Self {
        let token = market.token(direction);
        Self {
            execution_state: ExecutionState::AwaitingReconciliation,
            market_topic_id: market.market_topic_id,
            market_id: token.market_id,
            token_id: token.token_id.clone(),
            market_slug: market.slug.clone(),
            direction,
            order_id,
            requested_usdt,
            entry_price: Decimal::ZERO,
            filled_shares: Decimal::ZERO,
            trade_cost: Decimal::ZERO,
            fee: Decimal::ZERO,
            settlement_time_ms: market.end_ms,
            entry_btc_price,
        }
    }

    pub fn total_cost(&self) -> Decimal {
        self.trade_cost + self.fee
    }
}

#[derive(Debug, Clone)]
pub struct SettlementResult {
    pub market_topic_id: i64,
    pub market_slug: String,
    pub token_id: String,
    pub direction: Direction,
    pub payout: Decimal,
    pub pnl: Decimal,
    pub won: bool,
    pub entry_btc_price: Decimal,
}

pub struct Settler {
    pending: HashMap<i64, PendingPosition>,
}

impl Default for Settler {
    fn default() -> Self {
        Self::new()
    }
}

impl Settler {
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
        }
    }

    pub async fn load(log_dir: &str) -> anyhow::Result<Self> {
        let path = Path::new(log_dir).join("pending_positions.json");
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Self::new()),
            Err(error) => return Err(error).context("failed to read pending position state"),
        };
        let positions: Vec<PendingPosition> =
            serde_json::from_slice(&bytes).context("failed to parse pending position state")?;
        let mut tracker = Self::new();
        for position in positions {
            tracker.add(position);
        }
        Ok(tracker)
    }

    pub async fn persist(&self, log_dir: &str) -> anyhow::Result<()> {
        let directory = Path::new(log_dir);
        let temporary = directory.join("pending_positions.json.tmp");
        let destination = directory.join("pending_positions.json");
        let mut positions: Vec<_> = self.pending.values().collect();
        positions.sort_by_key(|position| position.market_topic_id);
        tokio::fs::write(
            &temporary,
            serde_json::to_vec_pretty(&positions)
                .context("failed to serialize pending positions")?,
        )
        .await
        .context("failed to write pending positions")?;
        tokio::fs::rename(&temporary, &destination)
            .await
            .context("failed to replace pending positions")?;
        Ok(())
    }

    pub fn add(&mut self, position: PendingPosition) {
        if self.pending.contains_key(&position.market_topic_id) {
            tracing::warn!(
                "[POSITION] duplicate Binance Prediction market {} ignored",
                position.market_topic_id
            );
            return;
        }
        self.pending.insert(position.market_topic_id, position);
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn has_market(&self, market_topic_id: i64) -> bool {
        self.pending.contains_key(&market_topic_id)
    }

    pub fn awaiting_reconciliation(&self) -> Vec<PendingPosition> {
        self.pending
            .values()
            .filter(|position| position.execution_state == ExecutionState::AwaitingReconciliation)
            .cloned()
            .collect()
    }

    pub fn due_positions(&self) -> Vec<PendingPosition> {
        let now_ms = Utc::now().timestamp_millis();
        self.pending
            .values()
            .filter(|position| {
                position.execution_state == ExecutionState::Open
                    && position.settlement_time_ms <= now_ms
            })
            .cloned()
            .collect()
    }

    pub fn mark_filled(
        &mut self,
        market_topic_id: i64,
        order: OrderResult,
    ) -> Option<PendingPosition> {
        let position = self.pending.get_mut(&market_topic_id)?;
        if position.direction != order.direction
            || order.filled_shares <= Decimal::ZERO
            || order.trade_cost <= Decimal::ZERO
        {
            tracing::error!(
                "[POSITION] Binance reconciliation mismatch for market {}",
                market_topic_id
            );
            return None;
        }
        position.execution_state = ExecutionState::Open;
        position.entry_price = order.entry_price;
        position.filled_shares = order.filled_shares;
        position.trade_cost = order.trade_cost;
        position.fee = order.fee;
        position.order_id = order.order_id;
        Some(position.clone())
    }

    pub fn remove_unfilled(&mut self, market_topic_id: i64) -> Option<PendingPosition> {
        self.pending.remove(&market_topic_id)
    }

    pub fn settle_paper(
        &mut self,
        market_topic_id: i64,
        winner: Direction,
    ) -> Option<SettlementResult> {
        let position = self.pending.remove(&market_topic_id)?;
        debug_assert_eq!(position.execution_state, ExecutionState::Open);
        let won = position.direction == winner;
        let payout = if won {
            position.filled_shares
        } else {
            Decimal::ZERO
        };
        let pnl = payout - position.total_cost();
        Some(Self::settlement_result(position, won, payout, pnl))
    }

    pub fn settle_live(
        &mut self,
        market_topic_id: i64,
        won: bool,
        payout: Decimal,
        pnl: Decimal,
    ) -> Option<SettlementResult> {
        let position = self.pending.remove(&market_topic_id)?;
        debug_assert_eq!(position.execution_state, ExecutionState::Open);
        Some(Self::settlement_result(position, won, payout, pnl))
    }

    fn settlement_result(
        position: PendingPosition,
        won: bool,
        payout: Decimal,
        pnl: Decimal,
    ) -> SettlementResult {
        tracing::info!(
            "[SETTLED] {} {} stake={:.2} pnl={:+.2}",
            if won { "WIN" } else { "LOSS" },
            position.direction.as_str(),
            position.total_cost(),
            pnl,
        );
        SettlementResult {
            market_topic_id: position.market_topic_id,
            market_slug: position.market_slug,
            token_id: position.token_id,
            direction: position.direction,
            payout,
            pnl,
            won,
            entry_btc_price: position.entry_btc_price,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::test_helpers::d;

    fn market() -> ActivePredictionMarket {
        ActivePredictionMarket {
            market_topic_id: 7,
            vendor: "PREDICT_FUN".into(),
            slug: "btc-five-minute-7".into(),
            title: "BTC 5m".into(),
            start_ms: 0,
            end_ms: 1,
            reference_price: d("64000"),
            fee_rate_bps: 200,
            up: crate::data::binance_prediction::MarketToken {
                market_id: 42,
                token_id: "up-token".into(),
            },
            down: crate::data::binance_prediction::MarketToken {
                market_id: 42,
                token_id: "down-token".into(),
            },
        }
    }

    fn order() -> OrderResult {
        OrderResult {
            order_id: "order".into(),
            direction: Direction::Up,
            requested_usdt: d("2"),
            entry_price: d("0.5"),
            filled_shares: d("4"),
            trade_cost: d("2"),
            fee: d("0.04"),
            settlement_time_ms: 1,
            entry_btc_price: d("64000"),
        }
    }

    #[test]
    fn settles_paper_using_filled_shares_and_fee_inclusive_cost() {
        let mut tracker = Settler::new();
        tracker.add(PendingPosition::from_filled(&market(), order()));
        let result = tracker.settle_paper(7, Direction::Up).unwrap();
        assert!(result.won);
        assert_eq!(result.payout, d("4"));
        assert_eq!(result.pnl, d("1.96"));
        assert_eq!(tracker.pending_count(), 0);
    }

    #[test]
    fn awaiting_execution_blocks_duplicate_market_until_reconciled() {
        let mut tracker = Settler::new();
        tracker.add(PendingPosition::awaiting_reconciliation(
            &market(),
            Direction::Up,
            d("2"),
            "order".into(),
            d("64000"),
        ));
        assert!(tracker.has_market(7));
        assert_eq!(tracker.awaiting_reconciliation().len(), 1);
        let filled = tracker.mark_filled(7, order()).unwrap();
        assert_eq!(filled.execution_state, ExecutionState::Open);
        assert!(tracker.awaiting_reconciliation().is_empty());
    }

    #[tokio::test]
    async fn pending_positions_survive_restart() {
        let directory = tempfile::tempdir().unwrap();
        let mut tracker = Settler::new();
        tracker.add(PendingPosition::from_filled(&market(), order()));
        tracker
            .persist(directory.path().to_str().unwrap())
            .await
            .unwrap();
        let restored = Settler::load(directory.path().to_str().unwrap())
            .await
            .unwrap();
        assert!(restored.has_market(7));
    }

    #[tokio::test]
    async fn corrupt_pending_state_fails_startup() {
        let directory = tempfile::tempdir().unwrap();
        tokio::fs::write(directory.path().join("pending_positions.json"), b"not-json")
            .await
            .unwrap();
        assert!(Settler::load(directory.path().to_str().unwrap())
            .await
            .is_err());
    }
}
