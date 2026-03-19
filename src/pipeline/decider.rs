//! Stage 3: Trade Decider
//! Market sentiment arbitrage decider.
//!
//! Core logic: When market is extremely overconfident (>80%), bet against it.
//! Edge = 0.50 - cheap_side_price (our fair value minus market's extreme price).
//! Direction is determined by market price extremes, with a BTC momentum filter
//! to avoid betting against strong short-term trends.

use crate::pipeline::signal::Direction;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

const PAUSE_SHORT_MS: i64 = 60_000;
const PAUSE_LONG_MS: i64 = 300_000;

#[derive(Debug, Clone)]
pub(crate) enum Decision {
    Pass(String),
    Trade {
        direction: Direction,
        size_usdc: Decimal,
        edge: Decimal,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct DeciderConfig {
    /// Minimum edge to trade (15%)
    pub edge_threshold: Decimal,
    /// Cooldown between trades (ms)
    pub cooldown_ms: i64,
    /// Market price threshold to consider "extreme" (e.g. 0.80)
    pub extreme_threshold: Decimal,
    /// Fair value assumption for binary outcome (e.g. 0.50)
    pub fair_value: Decimal,
    /// Maximum consecutive losses before circuit breaker
    pub max_consecutive_losses: u32,
    /// Maximum daily loss as fraction of balance (e.g. 0.10 = 10%)
    pub max_daily_loss_pct: Decimal,
    /// BTC momentum threshold to skip trade (e.g. 0.001 = 0.1%)
    pub momentum_threshold: Decimal,
    /// Momentum lookback window in milliseconds (e.g. 120_000 = 2 min)
    pub momentum_lookback_ms: i64,
}

impl Default for DeciderConfig {
    fn default() -> Self {
        Self {
            edge_threshold: decimal("0.15"),
            cooldown_ms: 5_000,
            extreme_threshold: decimal("0.80"),
            fair_value: decimal("0.50"),
            max_consecutive_losses: 8,
            max_daily_loss_pct: decimal("0.10"),
            momentum_threshold: decimal("0.001"),
            momentum_lookback_ms: 120_000,
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
    fn new() -> Self {
        Self { wins: 0, losses: 0 }
    }
}

fn decimal(value: &str) -> Decimal {
    Decimal::from_str_exact(value).expect("valid decimal literal")
}

#[derive(Debug, Clone)]
pub(crate) struct AccountState {
    pub balance: Decimal,
    pub consecutive_losses: u32,
    pub consecutive_wins: u32,
    pub total_wins: u32,
    pub total_losses: u32,
    pub last_trade_time_ms: i64,
    pub daily_pnl: Decimal,
    pub pnl_reset_date: String,
    up_stats: DirectionStats,
    down_stats: DirectionStats,
    pub last_traded_settlement_ms: i64,
    pub pause_until_ms: i64,
}

impl AccountState {
    pub(crate) fn new(balance: Decimal) -> Self {
        Self {
            balance,
            consecutive_losses: 0,
            consecutive_wins: 0,
            total_wins: 0,
            total_losses: 0,
            last_trade_time_ms: 0,
            daily_pnl: Decimal::ZERO,
            pnl_reset_date: chrono::Utc::now().format("%Y-%m-%d").to_string(),
            up_stats: DirectionStats::new(),
            down_stats: DirectionStats::new(),
            last_traded_settlement_ms: 0,
            pause_until_ms: 0,
        }
    }

    pub(crate) fn check_daily_reset(&mut self) {
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        if today != self.pnl_reset_date {
            tracing::info!(
                "[RISK] New day ({}), resetting daily_pnl from {:+.2}",
                today,
                self.daily_pnl
            );
            self.daily_pnl = Decimal::ZERO;
            self.pnl_reset_date = today;
        }
    }

    pub(crate) fn already_traded_market(&self, settlement_ms: i64) -> bool {
        self.last_traded_settlement_ms == settlement_ms && settlement_ms > 0
    }

    pub(crate) fn record_trade_for_market(&mut self, settlement_ms: i64) {
        self.last_traded_settlement_ms = settlement_ms;
    }

    fn can_trade(&self, cfg: &DeciderConfig) -> bool {
        if self.balance <= Decimal::ZERO {
            return false;
        }

        // Check cooldown
        let now = chrono::Utc::now().timestamp_millis();
        if now - self.last_trade_time_ms < cfg.cooldown_ms {
            return false;
        }

        // Check if we're in a pause period
        if now < self.pause_until_ms {
            return false;
        }

        // Hard stop: circuit breaker
        if self.consecutive_losses >= cfg.max_consecutive_losses {
            return false;
        }

        // Daily loss limit
        if self.daily_pnl <= -(self.balance * cfg.max_daily_loss_pct) {
            return false;
        }

        true
    }

    /// Check if we should pause after losses (trend detection)
    /// Returns pause duration in ms, or 0 if no pause needed
    fn loss_pause_duration(&self) -> i64 {
        match self.consecutive_losses {
            0..=3 => 0,              // No pause
            4..=5 => PAUSE_SHORT_MS, // 1 minute pause
            6..=7 => PAUSE_LONG_MS,  // 5 minutes pause
            _ => 0,                  // Hard stop handled elsewhere
        }
    }

    pub(crate) fn record_trade(&mut self, cost: Decimal) {
        self.balance -= cost;
        self.last_trade_time_ms = chrono::Utc::now().timestamp_millis();
    }

    pub(crate) fn record_settlement(
        &mut self,
        result: &crate::pipeline::settler::SettlementResult,
    ) {
        self.balance += result.payout;
        self.daily_pnl += result.pnl;

        if result.won {
            self.consecutive_wins += 1;
            self.consecutive_losses = 0;
            self.total_wins += 1;
            match result.direction {
                Direction::Up => self.up_stats.wins += 1,
                Direction::Down => self.down_stats.wins += 1,
            }
        } else {
            self.consecutive_losses += 1;
            self.consecutive_wins = 0;
            self.total_losses += 1;
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
                    self.consecutive_losses,
                    pause_ms / 1000
                );
            }
        }
    }
}

fn btc_momentum(prices: &[(f64, i64)], lookback_ms: i64) -> Option<f64> {
    if prices.len() < 2 {
        return None;
    }
    let (now_price, now_ts) = prices[prices.len() - 1];
    let cutoff = now_ts - lookback_ms;

    let past = prices
        .iter()
        .rev()
        .find(|(_, ts)| *ts <= cutoff)
        .map(|(p, _)| *p);

    let past_price = past?;
    if past_price <= 0.0 {
        return None;
    }
    Some((now_price - past_price) / past_price)
}

pub(crate) fn decide(
    market_yes: Option<Decimal>,
    market_no: Option<Decimal>,
    settlement_ms: i64,
    account: &AccountState,
    cfg: &DeciderConfig,
    btc_prices: &[(f64, i64)],
) -> Decision {
    // 1. One trade per market window
    if account.already_traded_market(settlement_ms) {
        return Decision::Pass("already_traded".into());
    }

    // 2. Risk check
    if !account.can_trade(cfg) {
        if account.consecutive_losses >= cfg.max_consecutive_losses {
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
        (Some(y), Some(n)) if y > decimal("0.01") && n > decimal("0.01") => (y, n),
        _ => return Decision::Pass("no_market_data".into()),
    };

    let total = yes + no;
    if total <= Decimal::ZERO {
        return Decision::Pass("no_liquidity".into());
    }

    let mkt_up = yes / total;

    // 4. Market extreme check + edge calculation
    let (edge, direction) = if mkt_up > cfg.extreme_threshold {
        let cheap_price = no / total;
        let edge = cfg.fair_value - cheap_price;
        (edge, Direction::Down)
    } else if mkt_up < (Decimal::ONE - cfg.extreme_threshold) {
        let cheap_price = yes / total;
        let edge = cfg.fair_value - cheap_price;
        (edge, Direction::Up)
    } else {
        return Decision::Pass(format!(
            "not_extreme_{}%",
            (mkt_up * decimal("100")).round_dp(0)
        ));
    };

    // 5. Edge threshold
    if edge < cfg.edge_threshold {
        return Decision::Pass(format!(
            "edge_{:.0}%<{:.0}%",
            edge.to_f64().unwrap_or(0.0) * 100.0,
            cfg.edge_threshold.to_f64().unwrap_or(0.0) * 100.0
        ));
    }

    if let Some(momentum) = btc_momentum(btc_prices, cfg.momentum_lookback_ms) {
        let momentum_threshold = cfg.momentum_threshold.to_f64().unwrap_or(0.0);
        let against_trend = match direction {
            Direction::Down => momentum > momentum_threshold,
            Direction::Up => momentum < -momentum_threshold,
        };
        if against_trend {
            return Decision::Pass(format!("against_trend_{:+.2}%", momentum * 100.0));
        }
    }

    let size = (account.balance / decimal("100")).max(decimal("1"));

    Decision::Trade {
        direction,
        size_usdc: size,
        edge,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(value: &str) -> rust_decimal::Decimal {
        rust_decimal::Decimal::from_str_exact(value).expect("valid decimal")
    }

    fn cfg_for_threshold_test() -> DeciderConfig {
        DeciderConfig {
            edge_threshold: d("0.15"),
            cooldown_ms: 5_000,
            extreme_threshold: d("0.64"),
            fair_value: d("0.50"),
            max_consecutive_losses: 8,
            max_daily_loss_pct: d("0.10"),
            momentum_threshold: d("0.001"),
            momentum_lookback_ms: 120_000,
        }
    }

    #[test]
    fn test_edge_equal_to_threshold_allows_trade() {
        let mut account = AccountState::new(d("1000"));
        account.last_trade_time_ms = chrono::Utc::now().timestamp_millis() - 60_000;

        let decision = decide(
            Some(d("0.65")),
            Some(d("0.35")),
            1_700_000_000_000,
            &account,
            &cfg_for_threshold_test(),
            &[(100000.0, 0), (100000.0, 120_000)],
        );

        match decision {
            Decision::Trade { edge, .. } => assert_eq!(edge, d("0.15")),
            Decision::Pass(reason) => panic!("expected trade but got pass: {}", reason),
        }
    }

    #[test]
    fn test_record_settlement_applies_decimal_pnl_exactly() {
        let mut account = AccountState::new(d("1000"));
        let result = crate::pipeline::settler::SettlementResult {
            direction: Direction::Up,
            payout: d("24.99"),
            pnl: d("19.99"),
            won: true,
            condition_id: "cid".into(),
            entry_btc_price: 70000.0,
        };

        account.record_trade(d("5.0"));
        account.record_settlement(&result);

        assert_eq!(account.balance, d("1019.99"));
        assert_eq!(account.daily_pnl, d("19.99"));
    }

    #[test]
    fn test_trade_when_extreme_bullish() {
        let account = AccountState::new(d("1000"));
        let cfg = DeciderConfig::default();

        let decision = decide(
            Some(d("0.85")),
            Some(d("0.15")),
            1_700_000_000_000,
            &account,
            &cfg,
            &[],
        );

        match decision {
            Decision::Trade {
                direction, edge, ..
            } => {
                assert_eq!(direction, Direction::Down);
                assert_eq!(edge, d("0.35"));
            }
            Decision::Pass(reason) => panic!("expected trade but got pass: {}", reason),
        }
    }

    #[test]
    fn test_position_size_is_one_percent_of_balance() {
        let mut account = AccountState::new(d("500"));
        account.last_trade_time_ms = chrono::Utc::now().timestamp_millis() - 60_000;
        let cfg = DeciderConfig::default();
        let decision = decide(
            Some(d("0.85")),
            Some(d("0.15")),
            1_700_000_000_000,
            &account,
            &cfg,
            &[],
        );
        match decision {
            Decision::Trade { size_usdc, .. } => assert_eq!(size_usdc, d("5")),
            Decision::Pass(reason) => panic!("expected trade but got pass: {}", reason),
        }
    }

    #[test]
    fn test_position_size_floor_at_one_usdc() {
        let mut account = AccountState::new(d("50"));
        account.last_trade_time_ms = chrono::Utc::now().timestamp_millis() - 60_000;
        let cfg = DeciderConfig::default();
        let decision = decide(
            Some(d("0.85")),
            Some(d("0.15")),
            1_700_000_000_000,
            &account,
            &cfg,
            &[],
        );
        match decision {
            Decision::Trade { size_usdc, .. } => assert_eq!(size_usdc, d("1")),
            Decision::Pass(reason) => panic!("expected trade but got pass: {}", reason),
        }
    }

    #[test]
    fn test_pass_when_not_extreme() {
        let account = AccountState::new(d("1000"));
        let cfg = DeciderConfig::default();

        let decision = decide(
            Some(d("0.55")),
            Some(d("0.45")),
            1_700_000_000_000,
            &account,
            &cfg,
            &[],
        );

        match decision {
            Decision::Pass(reason) => assert!(reason.starts_with("not_extreme_")),
            Decision::Trade { .. } => panic!("expected pass but got trade"),
        }
    }

    #[test]
    fn test_pass_when_already_traded_market() {
        let mut account = AccountState::new(d("1000"));
        let cfg = DeciderConfig::default();
        let settlement_ms = 1_700_000_000_000;
        account.record_trade_for_market(settlement_ms);

        let decision = decide(
            Some(d("0.85")),
            Some(d("0.15")),
            settlement_ms,
            &account,
            &cfg,
            &[],
        );

        match decision {
            Decision::Pass(reason) => assert_eq!(reason, "already_traded"),
            Decision::Trade { .. } => panic!("expected pass but got trade"),
        }
    }

    #[test]
    fn test_circuit_breaker_blocks_trading() {
        let mut account = AccountState::new(d("1000"));
        let cfg = DeciderConfig::default();
        account.consecutive_losses = cfg.max_consecutive_losses;

        let decision = decide(
            Some(d("0.85")),
            Some(d("0.15")),
            1_700_000_000_000,
            &account,
            &cfg,
            &[],
        );

        match decision {
            Decision::Pass(reason) => assert_eq!(reason, "circuit_breaker"),
            Decision::Trade { .. } => panic!("expected pass but got trade"),
        }
    }

    #[test]
    fn test_pass_when_no_market_data() {
        let account = AccountState::new(d("1000"));
        let cfg = DeciderConfig::default();

        let decision = decide(
            None,
            Some(d("0.15")),
            1_700_000_000_000,
            &account,
            &cfg,
            &[],
        );

        match decision {
            Decision::Pass(reason) => assert_eq!(reason, "no_market_data"),
            Decision::Trade { .. } => panic!("expected pass but got trade"),
        }
    }
}
