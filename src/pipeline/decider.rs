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

#[derive(Debug, Clone)]
pub(crate) enum Decision {
    Pass(String),
    Trade {
        direction: Direction,
        size_usdc: Decimal,
        edge: Decimal,
        /// (1 - cheap_price) / cheap_price — the core "以小搏大" metric.
        payoff_ratio: Decimal,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct DeciderConfig {
    /// Minimum edge to trade (15%)
    pub edge_threshold: Decimal,
    /// Maximum position size (USDC)
    pub max_position: Decimal,
    /// Minimum position size (USDC)
    pub min_position: Decimal,
    /// Cooldown between trades (ms)
    pub cooldown_ms: i64,
    /// Market price threshold to consider "extreme" (e.g. 0.80)
    pub extreme_threshold: Decimal,
    /// Fair value assumption for binary outcome (e.g. 0.50)
    pub fair_value: Decimal,
    /// Maximum daily loss as fraction of balance (e.g. 0.25 = 25%)
    pub max_daily_loss_pct: Decimal,
    /// BTC momentum threshold to skip trade (e.g. 0.001 = 0.1%)
    pub momentum_threshold: Decimal,
    /// Momentum lookback window in milliseconds (e.g. 120_000 = 2 min)
    pub momentum_lookback_ms: i64,
    /// Position size as percentage of balance (e.g. 1.0 = 1%)
    pub position_size_pct: Decimal,
}

impl Default for DeciderConfig {
    fn default() -> Self {
        Self {
            edge_threshold: decimal("0.15"),
            max_position: decimal("10.0"),
            min_position: decimal("1.0"),
            cooldown_ms: 5_000,
            extreme_threshold: decimal("0.80"),
            fair_value: decimal("0.50"),
            max_daily_loss_pct: decimal("0.25"),
            momentum_threshold: decimal("0.003"),
            momentum_lookback_ms: 120_000,
            position_size_pct: decimal("1.0"),
        }
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
    pub last_traded_settlement_ms: i64,
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
            last_traded_settlement_ms: 0,
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

    fn check_risk_controls(&self, cfg: &DeciderConfig) -> Option<&'static str> {
        let now = chrono::Utc::now().timestamp_millis();

        if self.balance <= Decimal::ZERO {
            tracing::error!("[RISK] Balance is zero or negative: {}, blocking trade", self.balance);
            return Some("insufficient_balance");
        }

        if now - self.last_trade_time_ms < cfg.cooldown_ms {
            let remaining = cfg.cooldown_ms - (now - self.last_trade_time_ms);
            tracing::debug!(
                "[RISK] Cooldown active: {}ms remaining, blocking trade",
                remaining
            );
            return Some("cooldown");
        }

        if self.daily_pnl <= -(self.balance * cfg.max_daily_loss_pct) {
            tracing::error!(
                "[RISK] Daily loss limit reached: pnl={:.2}, limit={:.2}, blocking trade",
                self.daily_pnl,
                -(self.balance * cfg.max_daily_loss_pct)
            );
            return Some("daily_loss_limit");
        }

        None
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
        } else {
            self.consecutive_losses += 1;
            self.consecutive_wins = 0;
            self.total_losses += 1;
        }
    }

}

fn btc_momentum(prices: &[(f64, i64)], lookback_ms: i64) -> Option<f64> {
    if prices.len() < 2 {
        return None;
    }
    let (now_price, now_ts) = prices[prices.len() - 1];
    let cutoff = now_ts - lookback_ms;

    let (past_price, past_ts) = prices
        .iter()
        .rev()
        .find(|(_, ts)| *ts <= cutoff)
        .copied()?;

    // Reject stale past price: if it's more than 2x the lookback window behind
    // the cutoff, there was a data gap (e.g. WS reconnect) and momentum is unreliable.
    if cutoff - past_ts > lookback_ms {
        return None;
    }

    if past_price <= 0.0 {
        return None;
    }
    Some((now_price - past_price) / past_price)
}

pub(crate) fn decide(
    market_yes: Option<Decimal>,
    market_no: Option<Decimal>,
    settlement_ms: i64,
    remaining_ms: i64,
    account: &AccountState,
    cfg: &DeciderConfig,
    btc_prices: &[(f64, i64)],
) -> Decision {
    // 1. One trade per market window
    if account.already_traded_market(settlement_ms) {
        return Decision::Pass("already_traded".into());
    }

    if let Some(reason) = account.check_risk_controls(cfg) {
        return Decision::Pass(reason.into());
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

    // Spread check: if yes + no < 0.80, liquidity is too thin and mid prices
    // are unreliable.  Skip to avoid adverse fills.
    if total < decimal("0.80") {
        return Decision::Pass(format!(
            "wide_spread_{:.0}%",
            ((Decimal::ONE - total) * decimal("100")).round_dp(0)
        ));
    }

    let mkt_up = yes / total;

    // 4. Market extreme check — time-weighted threshold.
    //    Early in window (>3min left): use configured threshold (e.g. 0.80).
    //    Late in window (<2min left):  require stronger extreme (0.90) because
    //    the market is more likely correct as outcome becomes clearer.
    let extreme_thr = if remaining_ms > 180_000 {
        cfg.extreme_threshold
    } else if remaining_ms > 120_000 {
        // Linear ramp from threshold → 0.90 between 3min and 2min
        let frac = Decimal::from(180_000 - remaining_ms) / Decimal::from(60_000_i64);
        cfg.extreme_threshold + (decimal("0.90") - cfg.extreme_threshold) * frac
    } else {
        decimal("0.90")
    };

    let (edge, direction) = if mkt_up > extreme_thr {
        let cheap_price = no / total;
        let edge = cfg.fair_value - cheap_price;
        (edge, Direction::Down)
    } else if mkt_up < (Decimal::ONE - extreme_thr) {
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

    // Momentum filter — relaxed for very cheap entries.
    // The cheaper the entry, the higher the payoff ratio, and the less we
    // need the market to be wrong.  At $0.10 (9x payoff) we only need 10%
    // win rate, so fighting a short-term trend is acceptable.
    //
    // Threshold scaling:  base threshold × payoff_ratio_factor
    //   payoff ≥ 9x  (price ≤ $0.10) → skip momentum filter entirely
    //   payoff 4-9x  (price $0.10-$0.20) → 3× looser threshold
    //   payoff < 4x  (price > $0.20) → normal threshold
    let cheap_price = match direction {
        Direction::Down => no / total,
        Direction::Up => yes / total,
    };
    let payoff_ratio = if cheap_price > Decimal::ZERO {
        (Decimal::ONE - cheap_price) / cheap_price
    } else {
        Decimal::new(99, 0)
    };

    let skip_momentum = payoff_ratio >= Decimal::new(9, 0); // ≥ 9x
    if !skip_momentum {
        match btc_momentum(btc_prices, cfg.momentum_lookback_ms) {
            None => {
                return Decision::Pass("no_momentum_data".into());
            }
            Some(momentum) => {
                let base_threshold = cfg.momentum_threshold.to_f64().unwrap_or(0.0);
                // Relax threshold for better payoff ratios (4x-9x → 3× looser)
                let effective_threshold = if payoff_ratio >= Decimal::new(4, 0) {
                    base_threshold * 3.0
                } else {
                    base_threshold
                };
                let against_trend = match direction {
                    Direction::Down => momentum > effective_threshold,
                    Direction::Up => momentum < -effective_threshold,
                };
                if against_trend {
                    return Decision::Pass(format!("against_trend_{:+.2}%", momentum * 100.0));
                }
            }
        }
    }

    // Position sizing: fixed % of balance, clamped to [min, max].
    // For asymmetric payoff strategies ("以小搏大"), each bet should be
    // consistently small.  Profit comes from the payoff ratio, not from
    // scaling position size.
    let size = (account.balance * cfg.position_size_pct / decimal("100"))
        .max(cfg.min_position)
        .min(cfg.max_position);

    Decision::Trade {
        direction,
        size_usdc: size,
        edge,
        payoff_ratio,
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
            extreme_threshold: d("0.64"),
            ..DeciderConfig::default()
        }
    }

    #[test]
    fn test_edge_equal_to_threshold_allows_trade() {
        let mut account = AccountState::new(d("1000"));
        account.last_trade_time_ms = chrono::Utc::now().timestamp_millis() - 60_000;

        // extreme_threshold=0.64, mkt_up=0.65 => direction=Down, need downward momentum
        let decision = decide(
            Some(d("0.65")),
            Some(d("0.35")),
            1_700_000_000_000,
            240_000,
            &account,
            &cfg_for_threshold_test(),
            &[(100400.0, 0), (100000.0, 120_000)],
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

    /// BTC prices with downward momentum (>0.3% drop) for confirming DOWN direction
    fn btc_down_momentum() -> Vec<(f64, i64)> {
        vec![(100400.0, 0), (100000.0, 120_000)]
    }

    #[test]
    fn test_trade_when_extreme_bullish() {
        let account = AccountState::new(d("1000"));
        let cfg = DeciderConfig::default();

        let decision = decide(
            Some(d("0.85")),
            Some(d("0.15")),
            1_700_000_000_000,
            240_000,
            &account,
            &cfg,
            &btc_down_momentum(),
        );

        match decision {
            Decision::Trade {
                direction,
                edge,
                payoff_ratio,
                ..
            } => {
                assert_eq!(direction, Direction::Down);
                assert_eq!(edge, d("0.35"));
                // cheap_price = 0.15, payoff = 0.85/0.15 ≈ 5.67
                assert!(payoff_ratio > d("5.66") && payoff_ratio < d("5.67"));
            }
            Decision::Pass(reason) => panic!("expected trade but got pass: {}", reason),
        }
    }

    #[test]
    fn test_position_size_is_fixed_pct_of_balance() {
        let mut account = AccountState::new(d("500"));
        account.last_trade_time_ms = chrono::Utc::now().timestamp_millis() - 60_000;
        let cfg = DeciderConfig::default();
        // 500 * 1% = 5, no edge scaling (以小搏大: fixed bet size)
        let decision = decide(
            Some(d("0.85")),
            Some(d("0.15")),
            1_700_000_000_000,
            240_000,
            &account,
            &cfg,
            &btc_down_momentum(),
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
        // 50 * 1% = 0.50, clamped to min 1
        let decision = decide(
            Some(d("0.85")),
            Some(d("0.15")),
            1_700_000_000_000,
            240_000,
            &account,
            &cfg,
            &btc_down_momentum(),
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
            240_000,
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
            240_000,
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
    fn test_cooldown_reason_string() {
        let mut account = AccountState::new(d("1000"));
        account.last_trade_time_ms = chrono::Utc::now().timestamp_millis(); // just traded

        let decision = decide(
            Some(d("0.85")),
            Some(d("0.15")),
            1_700_000_000_000,
            240_000,
            &account,
            &DeciderConfig::default(),
            &[(100400.0, 0), (100000.0, 120_000)],
        );

        match decision {
            Decision::Pass(r) => assert_eq!(r, "cooldown"),
            Decision::Trade { .. } => panic!("expected cooldown pass"),
        }
    }

    #[test]
    fn test_risk_controls_block_on_cooldown() {
        let mut account = AccountState::new(d("1000"));
        let cfg = DeciderConfig::default();
        account.last_trade_time_ms = chrono::Utc::now().timestamp_millis();

        let decision = decide(
            Some(d("0.85")),
            Some(d("0.15")),
            1_700_000_000_000,
            240_000,
            &account,
            &cfg,
            &btc_down_momentum(),
        );

        match decision {
            Decision::Pass(reason) => assert_eq!(reason, "cooldown"),
            Decision::Trade { .. } => {
                panic!("expected pass due to cooldown but got trade")
            }
        }
    }

    #[test]
    fn test_risk_controls_block_on_insufficient_balance() {
        let account = AccountState::new(d("0"));
        let cfg = DeciderConfig::default();

        let decision = decide(
            Some(d("0.85")),
            Some(d("0.15")),
            1_700_000_000_000,
            240_000,
            &account,
            &cfg,
            &btc_down_momentum(),
        );

        match decision {
            Decision::Pass(reason) => assert_eq!(reason, "insufficient_balance"),
            Decision::Trade { .. } => panic!("expected pass due to zero balance but got trade"),
        }
    }

    #[test]
    fn test_risk_controls_block_on_daily_loss_limit() {
        let mut account = AccountState::new(d("1000"));
        let cfg = DeciderConfig::default();
        account.last_trade_time_ms = chrono::Utc::now().timestamp_millis() - 60_000;
        account.daily_pnl = d("-300"); // exceeds 25% of $1000

        let decision = decide(
            Some(d("0.85")),
            Some(d("0.15")),
            1_700_000_000_000,
            240_000,
            &account,
            &cfg,
            &btc_down_momentum(),
        );

        match decision {
            Decision::Pass(reason) => assert_eq!(reason, "daily_loss_limit"),
            Decision::Trade { .. } => {
                panic!("expected pass due to daily loss limit but got trade")
            }
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
            240_000,
            &account,
            &cfg,
            &[],
        );

        match decision {
            Decision::Pass(reason) => assert_eq!(reason, "no_market_data"),
            Decision::Trade { .. } => panic!("expected pass but got trade"),
        }
    }

    #[test]
    fn test_pass_when_no_momentum_data() {
        let account = AccountState::new(d("1000"));
        let cfg = DeciderConfig::default();

        let decision = decide(
            Some(d("0.85")),
            Some(d("0.15")),
            1_700_000_000_000,
            240_000,
            &account,
            &cfg,
            &[],
        );

        match decision {
            Decision::Pass(reason) => assert_eq!(reason, "no_momentum_data"),
            Decision::Trade { .. } => panic!("expected pass but got trade"),
        }
    }
}
