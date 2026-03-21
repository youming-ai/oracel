//! Stage 5: Settler — track pending positions, settle at expiry.

use chrono::Utc;
use rust_decimal::Decimal;
use std::collections::VecDeque;

use crate::pipeline::signal::Direction;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct PendingPosition {
    pub direction: Direction,
    pub size_usdc: Decimal,
    pub entry_price: Decimal,
    pub filled_shares: Decimal,
    pub cost: Decimal,
    pub settlement_time_ms: i64,
    pub entry_btc_price: f64,
    pub condition_id: String,
    pub market_slug: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SettlementResult {
    pub direction: Direction,
    pub payout: Decimal,
    pub pnl: Decimal,
    pub won: bool,
    pub condition_id: String,
    pub entry_btc_price: f64,
}

pub(crate) struct Settler {
    pending: VecDeque<PendingPosition>,
}

impl Settler {
    pub(crate) fn new() -> Self {
        Self {
            pending: VecDeque::new(),
        }
    }

    pub(crate) fn restore_positions(&mut self, positions: Vec<PendingPosition>) {
        for pos in positions {
            // Skip duplicates based on condition_id
            if self
                .pending
                .iter()
                .any(|p| p.condition_id == pos.condition_id)
            {
                tracing::debug!(
                    "[SETTLER] Skipping duplicate position restore for {}",
                    pos.condition_id
                );
                continue;
            }
            self.pending.push_back(pos);
        }
    }

    pub(crate) fn pending_positions(&self) -> Vec<PendingPosition> {
        self.pending.iter().cloned().collect()
    }

    pub(crate) fn add_position(&mut self, pos: PendingPosition) {
        // Prevent duplicate positions for same market
        if self
            .pending
            .iter()
            .any(|p| p.condition_id == pos.condition_id)
        {
            tracing::warn!(
                "[SETTLER] Attempted to add duplicate position for {}",
                pos.condition_id
            );
            return;
        }
        self.pending.push_back(pos);
    }

    pub(crate) fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub(crate) fn due_positions(&self) -> Vec<PendingPosition> {
        let now = Utc::now().timestamp_millis();
        self.pending
            .iter()
            .filter(|p| p.settlement_time_ms <= now)
            .cloned()
            .collect()
    }

    pub(crate) fn settle_by_slug(&mut self, slug: &str, won: bool) -> Option<SettlementResult> {
        let matching: Vec<PendingPosition> = self
            .pending
            .iter()
            .filter(|p| p.market_slug == slug)
            .cloned()
            .collect();

        if matching.is_empty() {
            return None;
        }

        self.pending.retain(|p| p.market_slug != slug);

        let combined = self.combine_positions(matching, slug);
        Some(self.finish_settlement(combined, won))
    }

    fn combine_positions(&self, positions: Vec<PendingPosition>, slug: &str) -> PendingPosition {
        if positions.len() == 1 {
            if let Some(position) = positions.into_iter().next() {
                return position;
            }
            unreachable!("single-position combine must contain one position");
        }

        tracing::warn!(
            "[SETTLER] Settling {} positions for {}",
            positions.len(),
            slug
        );

        let first = positions
            .first()
            .unwrap_or_else(|| unreachable!("combine_positions requires at least one position"));
        PendingPosition {
            direction: first.direction,
            size_usdc: positions.iter().map(|p| p.size_usdc).sum(),
            entry_price: {
                let total_shares: Decimal = positions.iter().map(|p| p.filled_shares).sum();
                if total_shares > Decimal::ZERO {
                    positions.iter().map(|p| p.cost).sum::<Decimal>() / total_shares
                } else {
                    first.entry_price
                }
            },
            filled_shares: positions.iter().map(|p| p.filled_shares).sum(),
            cost: positions.iter().map(|p| p.cost).sum(),
            settlement_time_ms: first.settlement_time_ms,
            entry_btc_price: first.entry_btc_price,
            condition_id: first.condition_id.clone(),
            market_slug: first.market_slug.clone(),
        }
    }

    fn finish_settlement(&mut self, pos: PendingPosition, won: bool) -> SettlementResult {
        let payout = if won {
            pos.filled_shares
        } else {
            Decimal::ZERO
        };
        let pnl = payout - pos.cost;

        tracing::info!(
            "[SETTLED] {} {} stake={:.2} pnl={:+.2}",
            if won { "WIN" } else { "LOSS" },
            pos.direction.as_str(),
            pos.size_usdc.round_dp(2),
            pnl.round_dp(2),
        );

        SettlementResult {
            direction: pos.direction,
            payout,
            pnl,
            won,
            condition_id: pos.condition_id,
            entry_btc_price: pos.entry_btc_price,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(value: &str) -> rust_decimal::Decimal {
        rust_decimal::Decimal::from_str_exact(value).expect("valid decimal")
    }

    fn sample_pending() -> PendingPosition {
        PendingPosition {
            direction: Direction::Up,
            size_usdc: d("5.0"),
            entry_price: d("0.20"),
            filled_shares: d("25.00"),
            cost: d("5.0"),
            settlement_time_ms: 0,
            entry_btc_price: 70000.0,
            condition_id: "cid".into(),
            market_slug: "btc-updown-5m-1".into(),
        }
    }

    #[test]
    fn test_settle_by_slug_win() {
        let mut settler = Settler::new();
        settler.add_position(sample_pending());

        let result = settler.settle_by_slug("btc-updown-5m-1", true).unwrap();

        assert!(result.won);
        assert_eq!(result.payout, d("25.0"));
        assert_eq!(result.pnl, d("20.0"));
        assert_eq!(settler.pending_count(), 0);
    }

    #[test]
    fn test_settlement_uses_filled_shares_for_payout() {
        let mut settler = Settler::new();
        let mut pos = sample_pending();
        pos.filled_shares = d("24.99");
        settler.add_position(pos);

        let result = settler.settle_by_slug("btc-updown-5m-1", true).unwrap();

        assert_eq!(result.payout, d("24.99"));
        assert_eq!(result.pnl, d("19.99"));
    }

    #[test]
    fn test_settle_by_slug_none_when_empty() {
        let mut settler = Settler::new();
        assert!(settler.settle_by_slug("nonexistent", true).is_none());
    }

    #[test]
    fn test_add_position_prevents_duplicates() {
        let mut settler = Settler::new();
        let pos1 = sample_pending();
        let mut pos2 = sample_pending();
        pos2.size_usdc = d("10.0");

        settler.add_position(pos1);
        settler.add_position(pos2);

        assert_eq!(settler.pending_count(), 1);
    }

    #[test]
    fn test_restore_positions_dedupes() {
        let mut settler = Settler::new();
        let pos1 = sample_pending();
        let mut pos2 = sample_pending();
        pos2.size_usdc = d("10.0");

        settler.restore_positions(vec![pos1, pos2]);

        assert_eq!(settler.pending_count(), 1);
    }

    #[test]
    fn test_settle_by_slug_removes_all_duplicates() {
        let mut settler = Settler::new();
        let pos1 = sample_pending();
        let mut pos2 = sample_pending();
        pos2.condition_id = "cid2".to_string();

        settler.restore_positions(vec![pos1, pos2]);
        assert_eq!(settler.pending_count(), 2);

        let result = settler.settle_by_slug("btc-updown-5m-1", true).unwrap();

        assert_eq!(settler.pending_count(), 0);
        assert_eq!(result.payout, d("50.0"));
        assert_eq!(result.pnl, d("40.0"));
    }

    #[test]
    fn test_settle_by_slug_combines_cost_and_shares() {
        let mut settler = Settler::new();
        let pos1 = sample_pending();
        let mut pos2 = sample_pending();
        pos2.condition_id = "cid2".to_string();
        pos2.size_usdc = d("7.5");
        pos2.filled_shares = d("30.0");
        pos2.cost = d("7.5");

        settler.restore_positions(vec![pos1, pos2]);

        let result = settler.settle_by_slug("btc-updown-5m-1", true).unwrap();

        assert_eq!(result.payout, d("55.0"));
        assert_eq!(result.pnl, d("42.5"));
    }

    #[test]
    fn test_combine_positions_weighted_entry_price_value() {
        // 25 shares at 0.20 + 30 shares at 0.10 → weighted = 8.0 / 55 ≈ 0.1454
        // The combined position should NOT have entry_price = 0.20 (first only)
        let mut settler = Settler::new();

        let pos1 = PendingPosition {
            direction: Direction::Up,
            size_usdc: d("5.0"),
            entry_price: d("0.20"),
            filled_shares: d("25.0"),
            cost: d("5.0"),
            settlement_time_ms: 0,
            entry_btc_price: 70000.0,
            condition_id: "cid1".into(),
            market_slug: "slug".into(),
        };
        let pos2 = PendingPosition {
            direction: Direction::Up,
            size_usdc: d("3.0"),
            entry_price: d("0.10"),
            filled_shares: d("30.0"),
            cost: d("3.0"),
            settlement_time_ms: 0,
            entry_btc_price: 70000.0,
            condition_id: "cid2".into(),
            market_slug: "slug".into(),
        };

        settler.restore_positions(vec![pos1, pos2]);
        let result = settler.settle_by_slug("slug", true).unwrap();
        assert_eq!(result.pnl, d("47.0")); // 55 - 8 = 47
    }
}
