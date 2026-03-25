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
    pub min_entry_price: Decimal,
    pub max_entry_price: Decimal,
    pub min_ttl_for_entry_ms: u64,
    pub spot_momentum_30s_threshold: Decimal,
    pub spot_momentum_60s_threshold: Decimal,
    pub daily_loss_limit_usdc: Decimal,
}

impl Default for DeciderConfig {
    fn default() -> Self {
        Self {
            position_size_usdc: decimal("1.0"),
            extreme_threshold: decimal("0.80"),
            fair_value: decimal("0.50"),
            min_edge: decimal("0.05"),
            min_entry_price: decimal("0.08"),
            max_entry_price: decimal("0.12"),
            min_ttl_for_entry_ms: 120_000,
            spot_momentum_30s_threshold: decimal("40"),
            spot_momentum_60s_threshold: decimal("70"),
            daily_loss_limit_usdc: decimal("0"),
        }
    }
}

fn decimal(value: &str) -> Decimal {
    Decimal::from_str_exact(value).expect("valid decimal literal")
}

fn integer_suffix(value: Decimal) -> String {
    value.abs().trunc().to_string()
}

impl From<&crate::config::Config> for DeciderConfig {
    fn from(cfg: &crate::config::Config) -> Self {
        Self {
            position_size_usdc: cfg.strategy.position_size_usdc,
            extreme_threshold: cfg.strategy.extreme_threshold,
            fair_value: cfg.strategy.fair_value,
            min_edge: cfg.strategy.min_edge,
            min_entry_price: cfg.strategy.min_entry_price,
            max_entry_price: cfg.strategy.max_entry_price,
            min_ttl_for_entry_ms: cfg.strategy.min_ttl_for_entry_ms,
            spot_momentum_30s_threshold: cfg.strategy.spot_momentum_30s_threshold,
            spot_momentum_60s_threshold: cfg.strategy.spot_momentum_60s_threshold,
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
#[derive(Debug, Clone, Copy)]
pub(crate) struct SpotConfirmationContext {
    pub momentum_30s: Option<Decimal>,
    pub momentum_60s: Option<Decimal>,
}

pub(crate) struct DecideContext {
    pub market_yes: Option<Decimal>,
    pub market_no: Option<Decimal>,
    pub remaining_ms: i64,
    pub spot_confirmation: SpotConfirmationContext,
}

pub(crate) fn decide(ctx: &DecideContext, account: &AccountState, cfg: &DeciderConfig) -> Decision {
    if account.balance <= Decimal::ZERO {
        return Decision::Pass("insufficient_balance".into());
    }

    if cfg.daily_loss_limit_usdc > Decimal::ZERO && account.daily_pnl < -cfg.daily_loss_limit_usdc {
        return Decision::Pass(format!(
            "daily_loss_limit_{}",
            integer_suffix(account.daily_pnl)
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
            "edge_too_low_{}",
            integer_suffix(edge * decimal("100"))
        ));
    }

    if cheap_price < cfg.min_entry_price || cheap_price > cfg.max_entry_price {
        let cents = integer_suffix(cheap_price * decimal("100"));
        return Decision::Pass(format!("price_out_of_range_{cents}"));
    }

    let min_ttl_for_entry_ms = i64::try_from(cfg.min_ttl_for_entry_ms).unwrap_or(i64::MAX);
    if ctx.remaining_ms < min_ttl_for_entry_ms {
        let seconds = ctx.remaining_ms.unsigned_abs() / 1000;
        return Decision::Pass(format!("ttl_below_entry_floor_{seconds}"));
    }

    let spot_30s = match ctx.spot_confirmation.momentum_30s {
        Some(momentum) => momentum,
        None => return Decision::Pass("spot_confirmation_unavailable".into()),
    };
    let spot_60s = match ctx.spot_confirmation.momentum_60s {
        Some(momentum) => momentum,
        None => return Decision::Pass("spot_confirmation_unavailable".into()),
    };

    match base_direction {
        Direction::Down => {
            if spot_30s > cfg.spot_momentum_30s_threshold {
                return Decision::Pass(format!(
                    "spot_up_accelerating_30s_{}",
                    integer_suffix(spot_30s)
                ));
            }
            if spot_60s > cfg.spot_momentum_60s_threshold {
                return Decision::Pass(format!(
                    "spot_up_accelerating_60s_{}",
                    integer_suffix(spot_60s)
                ));
            }
        }
        Direction::Up => {
            if spot_30s < -cfg.spot_momentum_30s_threshold {
                return Decision::Pass(format!(
                    "spot_down_accelerating_30s_{}",
                    integer_suffix(spot_30s)
                ));
            }
            if spot_60s < -cfg.spot_momentum_60s_threshold {
                return Decision::Pass(format!(
                    "spot_down_accelerating_60s_{}",
                    integer_suffix(spot_60s)
                ));
            }
        }
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
            max_entry_price: d("0.50"),
            ..DeciderConfig::default()
        }
    }

    fn cfg_for_entry_filter_test() -> DeciderConfig {
        DeciderConfig {
            min_entry_price: d("0.08"),
            max_entry_price: d("0.12"),
            min_ttl_for_entry_ms: 120_000,
            spot_momentum_30s_threshold: d("40"),
            spot_momentum_60s_threshold: d("70"),
            ..DeciderConfig::default()
        }
    }

    fn default_ctx() -> DecideContext {
        DecideContext {
            market_yes: Some(d("0.85")),
            market_no: Some(d("0.15")),
            remaining_ms: 240_000,
            spot_confirmation: SpotConfirmationContext {
                momentum_30s: Some(Decimal::ZERO),
                momentum_60s: Some(Decimal::ZERO),
            },
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
        let cfg = DeciderConfig {
            max_entry_price: d("0.50"),
            ..DeciderConfig::default()
        };
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
            Decision::Pass(reason) => assert_eq!(reason, "daily_loss_limit_15"),
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
            Decision::Pass(reason) => assert_eq!(reason, "edge_too_low_35"),
            Decision::Trade { .. } => panic!("expected pass due to low edge"),
        }
    }

    #[test]
    fn test_min_edge_allows_high_edge() {
        let account = AccountState::new(d("1000"));
        let mut cfg = DeciderConfig {
            min_edge: d("0.05"),
            max_entry_price: d("0.50"),
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

    #[test]
    fn test_pass_when_entry_price_below_range_price_out_of_range() {
        let account = AccountState::new(d("1000"));
        let cfg = cfg_for_entry_filter_test();
        let mut ctx = default_ctx();
        ctx.market_yes = Some(d("0.93"));
        ctx.market_no = Some(d("0.07"));

        let decision = decide(&ctx, &account, &cfg);

        match decision {
            Decision::Pass(reason) => assert_eq!(reason, "price_out_of_range_7"),
            Decision::Trade { .. } => panic!("expected pass due to entry price below range"),
        }
    }

    #[test]
    fn test_pass_when_entry_price_above_range_price_out_of_range() {
        let account = AccountState::new(d("1000"));
        let cfg = cfg_for_entry_filter_test();
        let mut ctx = default_ctx();
        ctx.market_yes = Some(d("0.87"));
        ctx.market_no = Some(d("0.13"));

        let decision = decide(&ctx, &account, &cfg);

        match decision {
            Decision::Pass(reason) => assert_eq!(reason, "price_out_of_range_13"),
            Decision::Trade { .. } => panic!("expected pass due to entry price above range"),
        }
    }

    #[test]
    fn test_pass_when_remaining_ms_below_entry_floor_ttl_below_entry_floor() {
        let account = AccountState::new(d("1000"));
        let cfg = cfg_for_entry_filter_test();
        let mut ctx = default_ctx();
        ctx.market_yes = Some(d("0.91"));
        ctx.market_no = Some(d("0.09"));
        ctx.remaining_ms = 119_000;

        let decision = decide(&ctx, &account, &cfg);

        match decision {
            Decision::Pass(reason) => assert_eq!(reason, "ttl_below_entry_floor_119"),
            Decision::Trade { .. } => panic!("expected pass due to ttl floor"),
        }
    }

    #[test]
    fn test_pass_when_remaining_ms_negative_ttl_suffix_is_unsigned() {
        let account = AccountState::new(d("1000"));
        let cfg = cfg_for_entry_filter_test();
        let mut ctx = default_ctx();
        ctx.market_yes = Some(d("0.91"));
        ctx.market_no = Some(d("0.09"));
        ctx.remaining_ms = -1_000;

        let decision = decide(&ctx, &account, &cfg);

        match decision {
            Decision::Pass(reason) => assert_eq!(reason, "ttl_below_entry_floor_1"),
            Decision::Trade { .. } => panic!("expected pass due to ttl floor"),
        }
    }

    #[test]
    fn test_pass_when_down_trade_spot_still_accelerates_up_spot_up_accelerating() {
        let account = AccountState::new(d("1000"));
        let cfg = cfg_for_entry_filter_test();
        let mut ctx = default_ctx();
        ctx.market_yes = Some(d("0.90"));
        ctx.market_no = Some(d("0.10"));
        ctx.spot_confirmation = SpotConfirmationContext {
            momentum_30s: Some(d("45")),
            momentum_60s: Some(Decimal::ZERO),
        };

        let decision = decide(&ctx, &account, &cfg);

        match decision {
            Decision::Pass(reason) => assert_eq!(reason, "spot_up_accelerating_30s_45"),
            Decision::Trade { .. } => panic!("expected pass due to up acceleration"),
        }
    }

    #[test]
    fn test_pass_when_up_trade_spot_still_accelerates_down_spot_down_accelerating() {
        let account = AccountState::new(d("1000"));
        let cfg = cfg_for_entry_filter_test();
        let mut ctx = default_ctx();
        ctx.market_yes = Some(d("0.10"));
        ctx.market_no = Some(d("0.90"));
        ctx.spot_confirmation = SpotConfirmationContext {
            momentum_30s: Some(d("-45")),
            momentum_60s: Some(Decimal::ZERO),
        };

        let decision = decide(&ctx, &account, &cfg);

        match decision {
            Decision::Pass(reason) => assert_eq!(reason, "spot_down_accelerating_30s_45"),
            Decision::Trade { .. } => panic!("expected pass due to down acceleration"),
        }
    }

    #[test]
    fn test_pass_when_spot_confirmation_missing_spot_confirmation_unavailable() {
        let account = AccountState::new(d("1000"));
        let cfg = cfg_for_entry_filter_test();
        let mut ctx = default_ctx();
        ctx.market_yes = Some(d("0.90"));
        ctx.market_no = Some(d("0.10"));
        ctx.spot_confirmation = SpotConfirmationContext {
            momentum_30s: None,
            momentum_60s: Some(Decimal::ZERO),
        };

        let decision = decide(&ctx, &account, &cfg);

        match decision {
            Decision::Pass(reason) => assert_eq!(reason, "spot_confirmation_unavailable"),
            Decision::Trade { .. } => panic!("expected pass due to missing spot confirmation"),
        }
    }

    #[test]
    fn test_trade_when_spot_is_flat_or_countertrend() {
        let account = AccountState::new(d("1000"));
        let cfg = cfg_for_entry_filter_test();

        let mut down_ctx = default_ctx();
        down_ctx.market_yes = Some(d("0.90"));
        down_ctx.market_no = Some(d("0.10"));
        down_ctx.spot_confirmation = SpotConfirmationContext {
            momentum_30s: Some(d("-5")),
            momentum_60s: Some(d("0")),
        };

        let mut up_ctx = default_ctx();
        up_ctx.market_yes = Some(d("0.10"));
        up_ctx.market_no = Some(d("0.90"));
        up_ctx.spot_confirmation = SpotConfirmationContext {
            momentum_30s: Some(d("5")),
            momentum_60s: Some(d("0")),
        };

        match decide(&down_ctx, &account, &cfg) {
            Decision::Trade {
                direction: Direction::Down,
                ..
            } => {}
            Decision::Trade { direction, .. } => {
                panic!("expected down trade, got {:?}", direction)
            }
            Decision::Pass(reason) => {
                panic!("expected trade for down case but got pass: {}", reason)
            }
        }

        match decide(&up_ctx, &account, &cfg) {
            Decision::Trade {
                direction: Direction::Up,
                ..
            } => {}
            Decision::Trade { direction, .. } => panic!("expected up trade, got {:?}", direction),
            Decision::Pass(reason) => panic!("expected trade for up case but got pass: {}", reason),
        }
    }
}
