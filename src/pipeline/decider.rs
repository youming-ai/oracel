//! Stage 3: Trade Decider
//! Market sentiment arbitrage decider.
//!
//! Core logic: When market is extremely overconfident (>80%), bet against it.
//! Edge = 0.50 - cheap_side_price (our fair value minus market's extreme price).
//! Direction is determined by market price extremes.

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
    /// Fixed position size per trade (USDC)
    pub position_size_usdc: Decimal,
    /// Market price threshold to consider "extreme" (e.g. 0.80)
    pub extreme_threshold: Decimal,
    /// Fair value assumption for binary outcome (e.g. 0.50)
    pub fair_value: Decimal,
}

impl Default for DeciderConfig {
    fn default() -> Self {
        Self {
            edge_threshold: decimal("0.15"),
            position_size_usdc: decimal("1.0"),
            extreme_threshold: decimal("0.80"),
            fair_value: decimal("0.50"),
        }
    }
}

fn decimal(value: &str) -> Decimal {
    Decimal::from_str_exact(value).expect("valid decimal literal")
}

#[derive(Debug, Clone)]
pub(crate) struct AccountState {
    pub balance: Decimal,
    pub initial_balance: Decimal,
    pub consecutive_losses: u32,
    pub consecutive_wins: u32,
    pub total_wins: u32,
    pub total_losses: u32,
}

impl AccountState {
    pub(crate) fn new(balance: Decimal) -> Self {
        Self {
            balance,
            initial_balance: balance,
            consecutive_losses: 0,
            consecutive_wins: 0,
            total_wins: 0,
            total_losses: 0,
        }
    }

    pub(crate) fn pnl(&self) -> Decimal {
        self.balance - self.initial_balance
    }

    pub(crate) fn record_trade(&mut self, cost: Decimal) {
        self.balance -= cost;
    }

    pub(crate) fn record_settlement(
        &mut self,
        result: &crate::pipeline::settler::SettlementResult,
    ) {
        self.balance += result.payout;

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

pub(crate) fn decide(
    market_yes: Option<Decimal>,
    market_no: Option<Decimal>,
    _settlement_ms: i64,
    remaining_ms: i64,
    account: &AccountState,
    cfg: &DeciderConfig,
) -> Decision {
    // 1. Balance check
    if account.balance <= Decimal::ZERO {
        return Decision::Pass("insufficient_balance".into());
    }

    // 2. Need market data
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

    // 3. Market extreme check — time-weighted threshold.
    //    Early in window (>=3min left): use configured threshold (e.g. 0.80).
    //    Late in window (<2min left):   require stronger extreme (0.90) because
    //    the market is more likely correct as outcome becomes clearer.
    let extreme_thr = if remaining_ms > 180_000 {
        cfg.extreme_threshold
    } else if remaining_ms > 120_000 {
        // Linear ramp from threshold → 0.90 between 3min and 2min (exclusive)
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

    // 4. Edge threshold
    if edge < cfg.edge_threshold {
        return Decision::Pass(format!(
            "edge_{:.0}%<{:.0}%",
            edge.to_f64().unwrap_or(0.0) * 100.0,
            cfg.edge_threshold.to_f64().unwrap_or(0.0) * 100.0
        ));
    }

    // Calculate payoff ratio for logging
    let cheap_price = match direction {
        Direction::Down => no / total,
        Direction::Up => yes / total,
    };
    let payoff_ratio = if cheap_price > Decimal::ZERO {
        (Decimal::ONE - cheap_price) / cheap_price
    } else {
        Decimal::new(99, 0)
    };

    let size = cfg.position_size_usdc;

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
    use crate::pipeline::test_helpers::d;

    fn cfg_for_threshold_test() -> DeciderConfig {
        DeciderConfig {
            extreme_threshold: d("0.64"),
            ..DeciderConfig::default()
        }
    }

    #[test]
    fn test_edge_equal_to_threshold_allows_trade() {
        let account = AccountState::new(d("1000"));

        // extreme_threshold=0.64, mkt_up=0.65 => direction=Down
        let decision = decide(
            Some(d("0.65")),
            Some(d("0.35")),
            1_700_000_000_000,
            240_000,
            &account,
            &cfg_for_threshold_test(),
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
            entry_btc_price: d("70000"),
        };

        account.record_trade(d("5.0"));
        account.record_settlement(&result);

        assert_eq!(account.balance, d("1019.99"));
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
    fn test_position_size_is_fixed_one_dollar() {
        let account = AccountState::new(d("500"));
        let cfg = DeciderConfig::default();
        let decision = decide(
            Some(d("0.85")),
            Some(d("0.15")),
            1_700_000_000_000,
            240_000,
            &account,
            &cfg,
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
        );

        match decision {
            Decision::Pass(reason) => assert!(reason.starts_with("not_extreme_")),
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
            240_000,
            &account,
            &cfg,
        );

        match decision {
            Decision::Pass(reason) => assert_eq!(reason, "no_market_data"),
            Decision::Trade { .. } => panic!("expected pass but got trade"),
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
        );

        match decision {
            Decision::Pass(reason) => assert_eq!(reason, "insufficient_balance"),
            Decision::Trade { .. } => panic!("expected pass due to zero balance but got trade"),
        }
    }
}
