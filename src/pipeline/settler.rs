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
    total_wins: u32,
    total_losses: u32,
}

impl Settler {
    pub(crate) fn new() -> Self {
        Self {
            pending: VecDeque::new(),
            total_wins: 0,
            total_losses: 0,
        }
    }

    pub(crate) fn restore_positions(&mut self, positions: Vec<PendingPosition>) {
        for pos in positions {
            self.pending.push_back(pos);
        }
    }

    pub(crate) fn pending_positions(&self) -> Vec<PendingPosition> {
        self.pending.iter().cloned().collect()
    }

    pub(crate) fn add_position(&mut self, pos: PendingPosition) {
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
        let idx = self.pending.iter().position(|p| p.market_slug == slug)?;
        let pos = self.pending.remove(idx)?;
        Some(self.finish_settlement(pos, won))
    }

    pub(crate) fn check_settlements(
        &mut self,
        current_btc_price: f64,
        btc_tiebreaker_usd: f64,
    ) -> Vec<SettlementResult> {
        let now = Utc::now().timestamp_millis();
        let mut results = Vec::new();

        while let Some(pos) = self.pending.front() {
            if pos.settlement_time_ms > now {
                break;
            }
            let Some(pos) = self.pending.pop_front() else {
                break;
            };

            let btc_change = current_btc_price - pos.entry_btc_price;
            let btc_went_up = if btc_change.abs() < btc_tiebreaker_usd {
                // Tiebreaker: coin flip instead of biasing toward market sentiment
                pos.settlement_time_ms % 2 == 0
            } else {
                btc_change > 0.0
            };

            let won = match pos.direction {
                Direction::Up => btc_went_up,
                Direction::Down => !btc_went_up,
            };

            tracing::debug!("[SETTLEMENT] Local simulation - may not match Polymarket resolution");
            results.push(self.finish_settlement(pos, won));
        }

        results
    }

    fn finish_settlement(&mut self, pos: PendingPosition, won: bool) -> SettlementResult {
        let payout = if won {
            pos.filled_shares
        } else {
            Decimal::ZERO
        };
        let pnl = payout - pos.cost;

        if won {
            self.total_wins += 1;
        } else {
            self.total_losses += 1;
        }

        tracing::info!(
            "[SETTLED] {} {} stake={:.2} pnl={:+.2} {}W/{}L",
            if won { "WIN" } else { "LOSS" },
            pos.direction.as_str(),
            pos.size_usdc.round_dp(2),
            pnl.round_dp(2),
            self.total_wins,
            self.total_losses,
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
}
