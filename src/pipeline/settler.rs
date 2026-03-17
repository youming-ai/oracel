//! Stage 5: Settler
//! Tracks pending positions, settles them at expiry, updates balance.

use std::collections::VecDeque;
use chrono::Utc;
use serde_json;
use std::io::Write;

use crate::pipeline::signal::Direction;

#[derive(Debug, Clone)]
pub struct PendingPosition {
    pub order_id: String,
    pub direction: Direction,
    pub size_usdc: f64,
    pub entry_price: f64,
    pub cost: f64,
    pub token_id: String,
    pub settlement_time_ms: i64,
    pub entry_btc_price: f64,
}

#[derive(Debug, Clone)]
pub struct SettlementResult {
    pub order_id: String,
    pub direction: Direction,
    pub size_usdc: f64,
    pub cost: f64,
    pub payout: f64,
    pub pnl: f64,
    pub won: bool,
    pub entry_btc_price: f64,
    pub settle_btc_price: f64,
}

pub struct Settler {
    pending: VecDeque<PendingPosition>,
    total_wins: u32,
    total_losses: u32,
    total_pnl: f64,
}

impl Settler {
    pub fn new() -> Self {
        Self {
            pending: VecDeque::new(),
            total_wins: 0,
            total_losses: 0,
            total_pnl: 0.0,
        }
    }

    pub fn add_position(&mut self, pos: PendingPosition) {
        tracing::info!(
            "[SETTLEMENT] Tracking {} {} ${:.2} @ {:.3} | settles at {}",
            &pos.order_id[..8],
            pos.direction.as_str(),
            pos.size_usdc,
            pos.entry_price,
            chrono::DateTime::from_timestamp_millis(pos.settlement_time_ms)
                .map(|d| d.format("%H:%M:%S").to_string())
                .unwrap_or_else(|| "?".into()),
        );
        self.pending.push_back(pos);
    }

    /// Check and settle expired positions
    pub fn check_settlements(&mut self, current_btc_price: f64) -> Vec<SettlementResult> {
        let now = Utc::now().timestamp_millis();
        let mut results = Vec::new();

        while let Some(pos) = self.pending.front() {
            if pos.settlement_time_ms > now {
                break;
            }
            let pos = self.pending.pop_front().unwrap();

            // Determine outcome
            let btc_change = current_btc_price - pos.entry_btc_price;
            let btc_went_up = if btc_change.abs() < 5.0 {
                // Too close — use market price as tiebreaker
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
            self.total_pnl += pnl;

            let result = SettlementResult {
                order_id: pos.order_id.clone(),
                direction: pos.direction,
                size_usdc: pos.size_usdc,
                cost: pos.cost,
                payout,
                pnl,
                won,
                entry_btc_price: pos.entry_btc_price,
                settle_btc_price: current_btc_price,
            };

            tracing::info!(
                "[SETTLED] {} {} ${:.2} @ {:.3} | BTC ${:.0}→${:.0} | {} | PnL: ${:+.2} | {}W {}L",
                &pos.order_id[..8],
                pos.direction.as_str(),
                pos.size_usdc,
                pos.entry_price,
                pos.entry_btc_price,
                current_btc_price,
                if won { "✅ WIN" } else { "❌ LOSS" },
                pnl,
                self.total_wins,
                self.total_losses,
            );

            // Log to trade_log.json
            let entry = serde_json::json!({
                "timestamp": Utc::now().to_rfc3339(),
                "order_id": pos.order_id,
                "event": "settlement",
                "direction": pos.direction.as_str(),
                "size": pos.size_usdc,
                "cost": pos.cost,
                "payout": payout,
                "pnl": pnl,
                "won": won,
                "entry_btc": pos.entry_btc_price,
                "settle_btc": current_btc_price,
            });
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true).append(true).open("trade_log.json") {
                if let Err(e) = serde_json::to_writer(&file, &entry) {
                    tracing::warn!("Failed to write settlement log: {}", e);
                } else {
                    let _ = writeln!(file, "");
                }
            }

            results.push(result);
        }

        results
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn stats(&self) -> (u32, u32, f64) {
        (self.total_wins, self.total_losses, self.total_pnl)
    }
}
