//! Stage 5: Settler — track pending positions, settle at expiry.

use chrono::Utc;
use std::collections::VecDeque;
use std::io::Write;

use crate::pipeline::signal::Direction;

#[derive(Debug, Clone)]
pub struct PendingPosition {
    pub direction: Direction,
    pub size_usdc: f64,
    pub entry_price: f64,
    pub cost: f64,
    pub settlement_time_ms: i64,
    pub entry_btc_price: f64,
    pub condition_id: String,
}

#[derive(Debug, Clone)]
pub struct SettlementResult {
    pub direction: Direction,
    pub payout: f64,
    pub pnl: f64,
    pub won: bool,
    pub condition_id: String,
}

pub struct Settler {
    pending: VecDeque<PendingPosition>,
    total_wins: u32,
    total_losses: u32,
}

impl Settler {
    pub fn new() -> Self {
        Self { pending: VecDeque::new(), total_wins: 0, total_losses: 0 }
    }

    pub fn add_position(&mut self, pos: PendingPosition) {
        self.pending.push_back(pos);
    }

    pub fn check_settlements(
        &mut self,
        current_btc_price: f64,
        btc_tiebreaker_usd: f64,
    ) -> Vec<SettlementResult> {
        let now = Utc::now().timestamp_millis();
        let mut results = Vec::new();

        while let Some(pos) = self.pending.front() {
            if pos.settlement_time_ms > now { break; }
            let pos = self.pending.pop_front().unwrap();

            let btc_change = current_btc_price - pos.entry_btc_price;
            let btc_went_up = if btc_change.abs() < btc_tiebreaker_usd {
                pos.entry_price > 0.5
            } else {
                btc_change > 0.0
            };

            let won = match pos.direction {
                Direction::Up => btc_went_up,
                Direction::Down => !btc_went_up,
            };

            let payout = if won { pos.size_usdc } else { 0.0 };
            let pnl = payout - pos.cost;

            if won { self.total_wins += 1; } else { self.total_losses += 1; }

            tracing::info!(
                "[SETTLED] {} {} pnl={:+.2} {}W/{}L",
                if won { "WIN" } else { "LOSS" },
                pos.direction.as_str(), pnl, self.total_wins, self.total_losses,
            );

            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true).append(true)
                .open(std::path::Path::new("logs").join("trades.csv"))
            {
                let _ = writeln!(file, "{},{},{},{:+.2},{:.0},{:.0}",
                    Utc::now().format("%H:%M:%S"),
                    if won { "WIN" } else { "LOSS" },
                    pos.direction.as_str(),
                    pnl, pos.entry_btc_price, current_btc_price,
                );
            }

            results.push(SettlementResult { direction: pos.direction, payout, pnl, won, condition_id: pos.condition_id });
        }

        results
    }
}
