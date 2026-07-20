use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

#[derive(Debug, Clone)]
pub struct TradeRow {
    pub time: DateTime<Utc>,
    pub direction: String,
    pub entry_price: Decimal,
    pub cost: Decimal,
    pub edge: Decimal,
    pub result: String,
    pub pnl: Option<Decimal>,
}

pub struct TuiState {
    pub mode: String,
    pub btc_price: Decimal,
    pub market_slug: String,
    pub settlement_ms: i64,
    pub balance: Decimal,
    pub pnl: Decimal,
    pub total_wins: u32,
    pub total_losses: u32,
    pub consecutive_wins: u32,
    pub consecutive_losses: u32,
    pub pending_count: usize,
    pub last_decision: String,
    pub recent_trades: Vec<TradeRow>,
    pub scroll_offset: usize,
}

impl Default for TuiState {
    fn default() -> Self {
        Self {
            mode: String::new(),
            btc_price: Decimal::ZERO,
            market_slug: String::new(),
            settlement_ms: 0,
            balance: Decimal::ZERO,
            pnl: Decimal::ZERO,
            total_wins: 0,
            total_losses: 0,
            consecutive_wins: 0,
            consecutive_losses: 0,
            pending_count: 0,
            last_decision: String::new(),
            recent_trades: Vec::new(),
            scroll_offset: 0,
        }
    }
}

impl TuiState {
    pub fn update_from_account(
        &mut self,
        balance: Decimal,
        pnl: Decimal,
        wins: u32,
        losses: u32,
        consecutive_wins: u32,
        consecutive_losses: u32,
    ) {
        self.balance = balance;
        self.pnl = pnl;
        self.total_wins = wins;
        self.total_losses = losses;
        self.consecutive_wins = consecutive_wins;
        self.consecutive_losses = consecutive_losses;
    }

    pub fn update_market(&mut self, slug: &str, settlement_ms: i64) {
        self.market_slug = slug.to_string();
        self.settlement_ms = settlement_ms;
    }

    pub fn set_btc_price(&mut self, price: Decimal) {
        self.btc_price = price;
    }

    pub fn set_decision(&mut self, decision: String) {
        self.last_decision = decision;
    }

    pub fn set_pending_count(&mut self, count: usize) {
        self.pending_count = count;
    }

    pub fn add_trade(&mut self, row: TradeRow) {
        self.recent_trades.push(row);
        if self.recent_trades.len() > 200 {
            self.recent_trades.remove(0);
        }
    }

    pub fn settle_latest(&mut self, direction: &str, won: bool, pnl: Decimal) {
        if let Some(trade) = self
            .recent_trades
            .iter_mut()
            .rev()
            .find(|trade| trade.direction == direction && trade.result == "PENDING")
        {
            trade.result = if won { "WIN" } else { "LOSS" }.to_string();
            trade.pnl = Some(pnl);
        }
    }

    pub fn load_trades_from_csv(log_dir: &str) -> Vec<TradeRow> {
        let path = std::path::Path::new(log_dir).join("trades.csv");
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };

        let mut state = Self::default();
        for line in content.lines().skip(1) {
            let fields: Vec<&str> = line.split(',').collect();
            if fields.len() < 3 {
                continue;
            }

            match fields[1] {
                "ENTRY" if fields.len() >= 8 => {
                    let time = chrono::DateTime::parse_from_rfc3339(fields[0])
                        .ok()
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now);
                    state.add_trade(TradeRow {
                        time,
                        direction: fields[2].to_string(),
                        entry_price: fields[4].parse().unwrap_or_default(),
                        cost: fields[5].parse().unwrap_or_default(),
                        edge: fields[6].parse().unwrap_or_default(),
                        result: "PENDING".to_string(),
                        pnl: None,
                    });
                }
                result @ ("WIN" | "LOSS") if fields.len() >= 5 => {
                    state.settle_latest(
                        fields[2],
                        result == "WIN",
                        fields[4].parse().unwrap_or_default(),
                    );
                }
                _ => {}
            }
        }
        state.recent_trades
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_csv_applies_settlement_to_entry() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(
            dir.path().join("trades.csv"),
            concat!(
                "timestamp,type,direction,order_id,entry_price,cost,edge,balance,remaining_ms,yes_price,no_price,payoff_ratio\n",
                "2026-01-01T00:00:00Z,ENTRY,UP,12345678,0.05,1.00,45.0,99.00,120s,0.05,0.95,19.0x\n",
                "2026-01-01T00:05:00Z,WIN,UP,,+19.00,70000,70100\n",
            ),
        )
        .expect("write CSV");

        let rows = TuiState::load_trades_from_csv(dir.path().to_str().expect("utf-8 path"));

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].result, "WIN");
        assert_eq!(rows[0].pnl, Some(Decimal::from(19)));
    }
}
