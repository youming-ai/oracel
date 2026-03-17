//! Stage 3: Trade Decider
//! Market sentiment arbitrage decider.
//!
//! Core logic: When market is extremely overconfident (>80%), bet against it.
//! Edge = 0.50 - cheap_side_price (our fair value minus market's extreme price).
//! Direction is determined purely by market price, not BTC trend.

use crate::pipeline::signal::{Signal, Direction};

#[derive(Debug, Clone)]
pub enum Decision {
    Pass(String),
    Trade {
        direction: Direction,
        size_usdc: f64,
        edge: f64,
    },
}

#[derive(Debug, Clone)]
pub struct DeciderConfig {
    /// Minimum edge to trade (15%)
    pub edge_threshold: f64,
    /// Maximum position size
    pub max_position: f64,
    /// Minimum position size
    pub min_position: f64,
    /// Cooldown between trades (ms)
    pub cooldown_ms: i64,
    /// Account balance fraction to risk per trade (Half-Kelly cap)
    pub max_risk_fraction: f64,
}

impl Default for DeciderConfig {
    fn default() -> Self {
        Self {
            edge_threshold: 0.15,   // 15% minimum edge
            max_position: 50.0,
            min_position: 5.0,
            cooldown_ms: 5_000,
            max_risk_fraction: 0.10,  // Max 10% of balance per trade
        }
    }
}

/// Track win/loss per direction
#[derive(Debug, Clone)]
struct DirectionStats {
    wins: u32,
    losses: u32,
}

impl DirectionStats {
    fn new() -> Self { Self { wins: 0, losses: 0 } }
    fn total(&self) -> u32 { self.wins + self.losses }
    fn win_rate(&self) -> f64 {
        let t = self.total();
        if t == 0 { return 0.5; }
        self.wins as f64 / t as f64
    }
}

#[derive(Debug, Clone)]
pub struct AccountState {
    pub balance: f64,
    pub consecutive_losses: u32,
    pub consecutive_wins: u32,
    pub last_trade_time_ms: i64,
    pub daily_pnl: f64,
    up_stats: DirectionStats,
    down_stats: DirectionStats,
    pub last_traded_settlement_ms: i64,
    /// Timestamp when we started pausing (0 = not pausing)
    pub pause_until_ms: i64,
}

impl AccountState {
    pub fn new(balance: f64) -> Self {
        Self {
            balance,
            consecutive_losses: 0,
            consecutive_wins: 0,
            last_trade_time_ms: 0,
            daily_pnl: 0.0,
            up_stats: DirectionStats::new(),
            down_stats: DirectionStats::new(),
            last_traded_settlement_ms: 0,
            pause_until_ms: 0,
        }
    }

    pub fn already_traded_market(&self, settlement_ms: i64) -> bool {
        self.last_traded_settlement_ms == settlement_ms && settlement_ms > 0
    }

    pub fn record_trade_for_market(&mut self, settlement_ms: i64) {
        self.last_traded_settlement_ms = settlement_ms;
    }

    fn can_trade(&self, cfg: &DeciderConfig) -> bool {
        if self.balance <= 0.0 { return false; }
        
        // Check cooldown
        let now = chrono::Utc::now().timestamp_millis();
        if now - self.last_trade_time_ms < cfg.cooldown_ms { return false; }
        
        // Check if we're in a pause period
        if now < self.pause_until_ms { return false; }
        
        // Hard stop: 8 consecutive losses (circuit breaker)
        if self.consecutive_losses >= 8 { return false; }
        
        // Daily loss limit: -10% of balance
        if self.daily_pnl <= -self.balance * 0.10 { return false; }
        
        true
    }

    /// Check if we should pause after losses (trend detection)
    /// Returns pause duration in ms, or 0 if no pause needed
    fn loss_pause_duration(&self) -> i64 {
        match self.consecutive_losses {
            0..=3 => 0,           // No pause
            4..=5 => 60_000,      // 1 minute pause
            6..=7 => 300_000,     // 5 minutes pause
            _ => 0,               // Hard stop handled elsewhere
        }
    }

    pub fn record_trade(&mut self, cost: f64) {
        self.balance -= cost;
        self.last_trade_time_ms = chrono::Utc::now().timestamp_millis();
    }

    pub fn record_settlement(&mut self, result: &crate::pipeline::settler::SettlementResult) {
        self.balance += result.payout;
        self.daily_pnl += result.pnl;
        
        if result.won {
            self.consecutive_wins += 1;
            self.consecutive_losses = 0;
            match result.direction {
                Direction::Up => self.up_stats.wins += 1,
                Direction::Down => self.down_stats.wins += 1,
            }
        } else {
            self.consecutive_losses += 1;
            self.consecutive_wins = 0;
            match result.direction {
                Direction::Up => self.up_stats.losses += 1,
                Direction::Down => self.down_stats.losses += 1,
            }
            
            // Set pause if needed
            let pause_ms = self.loss_pause_duration();
            if pause_ms > 0 {
                self.pause_until_ms = chrono::Utc::now().timestamp_millis() + pause_ms;
                tracing::warn!(
                    "[RISK] {} consecutive losses, pausing for {}s",
                    self.consecutive_losses, pause_ms / 1000
                );
            }
        }
    }

    /// Overall win rate across all trades
    fn overall_win_rate(&self) -> f64 {
        let total_wins = self.up_stats.wins + self.down_stats.wins;
        let total = self.up_stats.total() + self.down_stats.total();
        if total == 0 { return 0.5; }
        total_wins as f64 / total as f64
    }
}

pub fn decide(
    signal: &Signal,
    market_yes: Option<f64>,
    market_no: Option<f64>,
    settlement_ms: i64,
    account: &AccountState,
    cfg: &DeciderConfig,
) -> Decision {
    // 1. One trade per market window
    if account.already_traded_market(settlement_ms) {
        return Decision::Pass("already_traded".into());
    }

    // 2. Risk check
    if !account.can_trade(cfg) {
        if account.consecutive_losses >= 8 {
            return Decision::Pass("circuit_breaker".into());
        }
        if chrono::Utc::now().timestamp_millis() < account.pause_until_ms {
            let remaining = (account.pause_until_ms - chrono::Utc::now().timestamp_millis()) / 1000;
            return Decision::Pass(format!("loss_pause_{}s", remaining));
        }
        return Decision::Pass("risk_blocked".into());
    }

    // 3. Need market data
    let (yes, no) = match (market_yes, market_no) {
        (Some(y), Some(n)) if y > 0.01 && n > 0.01 => (y, n),
        _ => return Decision::Pass("no_market_data".into()),
    };

    let total = yes + no;
    if total <= 0.0 { return Decision::Pass("no_liquidity".into()); }
    
    let mkt_up = yes / total;
    
    // 4. Market extreme check + edge calculation
    const EXTREME_THRESHOLD: f64 = 0.80;
    const FAIR_VALUE: f64 = 0.50;
    
    let (edge, direction) = if mkt_up > EXTREME_THRESHOLD {
        // Market extremely bullish → bet DOWN
        let cheap_price = no / total;  // NO is cheap
        let edge = FAIR_VALUE - cheap_price;
        (edge, Direction::Down)
    } else if mkt_up < (1.0 - EXTREME_THRESHOLD) {
        // Market extremely bearish → bet UP
        let cheap_price = yes / total;  // YES is cheap
        let edge = FAIR_VALUE - cheap_price;
        (edge, Direction::Up)
    } else {
        return Decision::Pass(format!("not_extreme_{:.0}%", mkt_up * 100.0));
    };

    // 5. Edge threshold
    if edge < cfg.edge_threshold {
        return Decision::Pass(format!("edge_{:.0}%<{:.0}%", edge * 100.0, cfg.edge_threshold * 100.0));
    }

    // 6. Direction is determined purely by market price extremes.
    //    No BTC trend filter - the signal decides direction,
    //    not momentum. If market >80% confident, bet against it.

    // 7. Position sizing: Half-Kelly based on edge
    // Kelly = edge / (1 - edge) simplified for binary outcome
    // But we cap at max_risk_fraction
    let win_rate = account.overall_win_rate().clamp(0.50, 0.75);
    let kelly_fraction = (2.0 * win_rate - 1.0).max(0.05);
    let half_kelly = kelly_fraction * 0.5;
    
    // Scale by edge strength: 15% edge = 1x, 30% edge = 1.5x, 45%+ = 2x
    let edge_multiplier = (1.0 + (edge - 0.15) / 0.15).clamp(1.0, 2.0);
    
    let size = (account.balance * half_kelly * edge_multiplier)
        .clamp(cfg.min_position, cfg.max_position)
        .min(account.balance * cfg.max_risk_fraction);

    Decision::Trade { direction, size_usdc: size, edge }
}
