//! Stage 3: Trade Decider
//! Market sentiment arbitrage decider.
//!
//! Core logic: When market is extremely overconfident (>80%), bet against it.
//! Edge = fair_value - cheap_side_price (our fair value minus market's extreme price).
//! Direction is determined by market price extremes.
//! Risk controls: daily loss limit.

use crate::pipeline::signal::Direction;
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
    pub position_size_usdc: Decimal,
    pub extreme_threshold: Decimal,
    pub fair_value: Decimal,
    pub min_edge: Decimal,
    pub daily_loss_limit_usdc: Decimal,
}

impl Default for DeciderConfig {
    fn default() -> Self {
        Self {
            position_size_usdc: decimal("1.0"),
            extreme_threshold: decimal("0.80"),
            fair_value: decimal("0.50"),
            min_edge: decimal("0.05"),
            daily_loss_limit_usdc: decimal("0"),
        }
    }
}

fn decimal(value: &str) -> Decimal {
    Decimal::from_str_exact(value).expect("valid decimal literal")
}

impl From<&crate::config::Config> for DeciderConfig {
    fn from(cfg: &crate::config::Config) -> Self {
        Self {
            position_size_usdc: cfg.strategy.position_size_usdc,
            extreme_threshold: cfg.strategy.extreme_threshold,
            fair_value: cfg.strategy.fair_value,
            min_edge: cfg.strategy.min_edge,
            daily_loss_limit_usdc: cfg.risk.daily_loss_limit_usdc,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AccountState {
    pub balance: Decimal,
    pub initial_balance: Decimal,
    pub consecutive_losses: u32,
    pub consecutive_wins: u32,
    pub total_wins: u32,
    pub total_losses: u32,
    pub daily_pnl: Decimal,
    pub daily_reset_date: String,
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
            daily_pnl: Decimal::ZERO,
            daily_reset_date: String::new(),
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

    pub(crate) fn reset_daily_if_needed(&mut self, today: &str) {
        if self.daily_reset_date != today {
            self.daily_pnl = Decimal::ZERO;
            self.daily_reset_date = today.to_string();
        }
    }
}

/// Input context for the decide function
pub(crate) struct DecideContext {
    pub market_yes: Option<Decimal>,
    pub market_no: Option<Decimal>,
    pub remaining_ms: i64,
}

pub(crate) fn decide(
    ctx: &DecideContext,
    account: &AccountState,
    cfg: &DeciderConfig,
) -> Decision {
    if account.balance <= Decimal::ZERO {
        return Decision::Pass("insufficient_balance".into());
    }

    if cfg.daily_loss_limit_usdc > Decimal::ZERO && account.daily_pnl < -cfg.daily_loss_limit_usdc {
        return Decision::Pass(format!(
            "daily_loss_limit_{:.0}",
            account.daily_pnl.round_dp(0)
        ));
    }

    let (yes, no) = match (ctx.market_yes, ctx.market_no) {
        (Some(y), Some(n)) if y > decimal("0.01") && n > decimal("0.01") => (y, n),
        _ => return Decision::Pass("no_market_data".into()),
    };

    let total = yes + no;
    if total <= Decimal::ZERO {
        return Decision::Pass("no_liquidity".into());
    }

    if total < decimal("0.80") {
        return Decision::Pass(format!(
            "wide_spread_{:.0}%",
            ((Decimal::ONE - total) * decimal("100")).round_dp(0)
        ));
    }

    let mkt_up = yes / total;

    let late_floor = decimal("0.90");
    let late_threshold = if cfg.extreme_threshold > late_floor {
        cfg.extreme_threshold
    } else {
        late_floor
    };
    let extreme_thr = if ctx.remaining_ms > 180_000 {
        cfg.extreme_threshold
    } else if ctx.remaining_ms > 120_000 {
        let frac = Decimal::from(180_000 - ctx.remaining_ms) / Decimal::from(60_000_i64);
        cfg.extreme_threshold + (late_threshold - cfg.extreme_threshold) * frac
    } else {
        late_threshold
    };

    let (base_direction, cheap_price) = if mkt_up > extreme_thr {
        (Direction::Down, no / total)
    } else if mkt_up < (Decimal::ONE - extreme_thr) {
        (Direction::Up, yes / total)
    } else {
        return Decision::Pass(format!(
            "not_extreme_{}%",
            (mkt_up * decimal("100")).round_dp(0)
        ));
    };

    let edge = cfg.fair_value - cheap_price;

    if edge < cfg.min_edge {
        return Decision::Pass(format!(
            "edge_too_low_{:.1}%",
            (edge * decimal("100")).round_dp(1)
        ));
    }

    let payoff_ratio = if cheap_price > Decimal::ZERO {
        (Decimal::ONE - cheap_price) / cheap_price
    } else {
        Decimal::new(99, 0)
    };

    Decision::Trade {
        direction: base_direction,
        size_usdc: cfg.position_size_usdc,
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

    fn default_ctx() -> DecideContext {
        DecideContext {
            market_yes: Some(d("0.85")),
            market_no: Some(d("0.15")),
            remaining_ms: 240_000,
        }
    }

    #[test]
    fn test_edge_equal_to_threshold_allows_trade() {
        let account = AccountState::new(d("1000"));
        let mut ctx = default_ctx();
        ctx.market_yes = Some(d("0.65"));
        ctx.market_no = Some(d("0.35"));

        let decision = decide(&ctx, &account, &cfg_for_threshold_test());

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
        assert_eq!(account.daily_pnl, d("19.99"));
    }

    #[test]
    fn test_trade_when_extreme_bullish() {
        let account = AccountState::new(d("1000"));
        let cfg = DeciderConfig::default();
        let ctx = default_ctx();

        let decision = decide(&ctx, &account, &cfg);

        match decision {
            Decision::Trade {
                direction,
                edge,
                payoff_ratio,
                ..
            } => {
                assert_eq!(direction, Direction::Down);
                assert_eq!(edge, d("0.35"));
                assert!(payoff_ratio > d("5.66") && payoff_ratio < d("5.67"));
            }
            Decision::Pass(reason) => panic!("expected trade but got pass: {}", reason),
        }
    }

    #[test]
    fn test_daily_loss_limit() {
        let mut account = AccountState::new(d("1000"));
        account.daily_pnl = d("-15.0");
        let cfg = DeciderConfig {
            daily_loss_limit_usdc: d("10.0"),
            ..DeciderConfig::default()
        };
        let ctx = default_ctx();

        let decision = decide(&ctx, &account, &cfg);

        match decision {
            Decision::Pass(reason) => assert!(reason.starts_with("daily_loss_limit")),
            Decision::Trade { .. } => panic!("expected pass but got trade"),
        }
    }

    #[test]
    fn test_pass_when_not_extreme() {
        let account = AccountState::new(d("1000"));
        let cfg = DeciderConfig::default();
        let mut ctx = default_ctx();
        ctx.market_yes = Some(d("0.55"));
        ctx.market_no = Some(d("0.45"));

        let decision = decide(&ctx, &account, &cfg);

        match decision {
            Decision::Pass(reason) => assert!(reason.starts_with("not_extreme_")),
            Decision::Trade { .. } => panic!("expected pass but got trade"),
        }
    }

    #[test]
    fn test_pass_when_no_market_data() {
        let account = AccountState::new(d("1000"));
        let cfg = DeciderConfig::default();
        let mut ctx = default_ctx();
        ctx.market_yes = None;

        let decision = decide(&ctx, &account, &cfg);

        match decision {
            Decision::Pass(reason) => assert_eq!(reason, "no_market_data"),
            Decision::Trade { .. } => panic!("expected pass but got trade"),
        }
    }

    #[test]
    fn test_risk_controls_block_on_insufficient_balance() {
        let account = AccountState::new(d("0"));
        let cfg = DeciderConfig::default();
        let ctx = default_ctx();

        let decision = decide(&ctx, &account, &cfg);

        match decision {
            Decision::Pass(reason) => assert_eq!(reason, "insufficient_balance"),
            Decision::Trade { .. } => panic!("expected pass due to zero balance but got trade"),
        }
    }

    #[test]
    fn test_min_edge_rejects_low_edge() {
        let account = AccountState::new(d("1000"));
        let mut cfg = DeciderConfig {
            min_edge: d("0.50"),
            ..DeciderConfig::default()
        };
        cfg.extreme_threshold = d("0.64");

        let mut ctx = default_ctx();
        ctx.market_yes = Some(d("0.85"));
        ctx.market_no = Some(d("0.15"));

        let decision = decide(&ctx, &account, &cfg);

        match decision {
            Decision::Pass(reason) => assert!(reason.starts_with("edge_too_low")),
            Decision::Trade { .. } => panic!("expected pass due to low edge"),
        }
    }

    #[test]
    fn test_min_edge_allows_high_edge() {
        let account = AccountState::new(d("1000"));
        let mut cfg = DeciderConfig {
            min_edge: d("0.05"),
            ..DeciderConfig::default()
        };
        cfg.extreme_threshold = d("0.64");

        let mut ctx = default_ctx();
        ctx.market_yes = Some(d("0.70"));
        ctx.market_no = Some(d("0.30"));

        let decision = decide(&ctx, &account, &cfg);

        match decision {
            Decision::Trade { .. } => {}
            Decision::Pass(reason) => panic!("expected trade but got pass: {}", reason),
        }
    }
}
