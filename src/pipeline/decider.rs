//! Stage 3: Trade Decider
//! Market sentiment arbitrage decider.
//!
//! Core logic: When market is extremely overconfident (>80%), bet against it.
//! Edge = 0.50 - cheap_side_price (our fair value minus market's extreme price).
//! Direction is determined by market price extremes.
//!
//! Enhanced features:
//! - BTC momentum filter: skip trades when momentum opposes direction
//! - Dynamic fair value: adjust based on BTC volatility
//! - Risk controls: consecutive loss limit, daily loss limit

use crate::pipeline::price_source::PriceTick;
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
        /// BTC momentum at entry (for logging)
        btc_momentum: Decimal,
        /// BTC volatility at entry (for logging)
        btc_volatility: Decimal,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct DeciderConfig {
    /// Fixed position size per trade (USDC)
    pub position_size_usdc: Decimal,
    /// Market price threshold to consider "extreme" (e.g. 0.80)
    pub extreme_threshold: Decimal,
    /// Fair value assumption for binary outcome (e.g. 0.50)
    pub fair_value: Decimal,
    /// Momentum filter settings
    pub momentum_filter_enabled: bool,
    pub momentum_window_secs: u64,
    pub momentum_threshold: Decimal,
    /// Dynamic fair value settings
    pub dynamic_fv_enabled: bool,
    pub volatility_window_secs: u64,
    pub volatility_weight: Decimal,
    /// Risk controls
    pub max_consecutive_losses: u32,
    pub daily_loss_limit_usdc: Decimal,
}

impl Default for DeciderConfig {
    fn default() -> Self {
        Self {
            position_size_usdc: decimal("1.0"),
            extreme_threshold: decimal("0.80"),
            fair_value: decimal("0.50"),
            momentum_filter_enabled: false,
            momentum_window_secs: 60,
            momentum_threshold: decimal("0.002"),
            dynamic_fv_enabled: false,
            volatility_window_secs: 300,
            volatility_weight: decimal("0.1"),
            max_consecutive_losses: 5,
            daily_loss_limit_usdc: decimal("0"),
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

/// Compute BTC momentum as percentage change over window
pub(crate) fn compute_momentum(prices: &[PriceTick], window_secs: u64) -> Decimal {
    if prices.len() < 2 {
        return Decimal::ZERO;
    }

    let now_ms = prices.last().map(|p| p.timestamp_ms).unwrap_or(0);
    let cutoff_ms = now_ms - (window_secs as i64 * 1000);

    // Find earliest price within window
    let start_price = prices
        .iter()
        .find(|p| p.timestamp_ms >= cutoff_ms)
        .map(|p| p.price)
        .unwrap_or_else(|| prices.first().map(|p| p.price).unwrap_or(Decimal::ZERO));

    let end_price = prices.last().map(|p| p.price).unwrap_or(Decimal::ZERO);

    if start_price == Decimal::ZERO {
        return Decimal::ZERO;
    }

    (end_price - start_price) / start_price
}

/// Compute BTC volatility as standard deviation of returns over window
pub(crate) fn compute_volatility(prices: &[PriceTick], window_secs: u64) -> Decimal {
    if prices.len() < 3 {
        return Decimal::ZERO;
    }

    let now_ms = prices.last().map(|p| p.timestamp_ms).unwrap_or(0);
    let cutoff_ms = now_ms - (window_secs as i64 * 1000);

    // Filter prices within window
    let window_prices: Vec<Decimal> = prices
        .iter()
        .filter(|p| p.timestamp_ms >= cutoff_ms)
        .map(|p| p.price)
        .collect();

    if window_prices.len() < 3 {
        return Decimal::ZERO;
    }

    // Compute returns
    let returns: Vec<Decimal> = window_prices
        .windows(2)
        .filter_map(|w| {
            if w[0] > Decimal::ZERO {
                Some((w[1] - w[0]) / w[0])
            } else {
                None
            }
        })
        .collect();

    if returns.is_empty() {
        return Decimal::ZERO;
    }

    // Compute mean
    let sum: Decimal = returns.iter().sum();
    let mean = sum / Decimal::from(returns.len() as i32);

    // Compute variance
    let variance: Decimal = returns
        .iter()
        .map(|r| {
            let diff = *r - mean;
            diff * diff
        })
        .sum::<Decimal>()
        / Decimal::from(returns.len() as i32);

    // Standard deviation via f64 sqrt (precise enough for volatility metric)
    if variance <= Decimal::ZERO {
        return Decimal::ZERO;
    }

    use rust_decimal::prelude::ToPrimitive;
    let std_dev = variance.to_f64().unwrap_or(0.0).sqrt();
    Decimal::try_from(std_dev).unwrap_or(Decimal::ZERO)
}

/// Input context for the decide function
pub(crate) struct DecideContext {
    pub market_yes: Option<Decimal>,
    pub market_no: Option<Decimal>,
    pub remaining_ms: i64,
    pub btc_prices: Vec<PriceTick>,
}

pub(crate) fn decide(ctx: &DecideContext, account: &AccountState, cfg: &DeciderConfig) -> Decision {
    // 1. Balance check
    if account.balance <= Decimal::ZERO {
        return Decision::Pass("insufficient_balance".into());
    }

    // 2. Consecutive loss circuit breaker
    if cfg.max_consecutive_losses > 0 && account.consecutive_losses >= cfg.max_consecutive_losses {
        return Decision::Pass(format!(
            "circuit_breaker_{}losses",
            account.consecutive_losses
        ));
    }

    // 3. Daily loss limit check
    if cfg.daily_loss_limit_usdc > Decimal::ZERO && account.daily_pnl < -cfg.daily_loss_limit_usdc {
        return Decision::Pass(format!(
            "daily_loss_limit_{:.0}",
            account.daily_pnl.round_dp(0)
        ));
    }

    // 4. Need market data
    let (yes, no) = match (ctx.market_yes, ctx.market_no) {
        (Some(y), Some(n)) if y > decimal("0.01") && n > decimal("0.01") => (y, n),
        _ => return Decision::Pass("no_market_data".into()),
    };

    let total = yes + no;
    if total <= Decimal::ZERO {
        return Decision::Pass("no_liquidity".into());
    }

    // Spread check: if yes + no < 0.80, liquidity is too thin
    if total < decimal("0.80") {
        return Decision::Pass(format!(
            "wide_spread_{:.0}%",
            ((Decimal::ONE - total) * decimal("100")).round_dp(0)
        ));
    }

    let mkt_up = yes / total;

    // 5. Compute BTC momentum and volatility
    let btc_momentum = compute_momentum(&ctx.btc_prices, cfg.momentum_window_secs);
    let btc_volatility = compute_volatility(&ctx.btc_prices, cfg.volatility_window_secs);

    // 6. Market extreme check — time-weighted threshold
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

    // 7. Determine direction based on market extreme
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

    // 8. Momentum filter: skip if momentum strongly opposes direction
    if cfg.momentum_filter_enabled && cfg.momentum_threshold > Decimal::ZERO {
        let momentum_opposes = (base_direction == Direction::Up
            && btc_momentum < -cfg.momentum_threshold)
            || (base_direction == Direction::Down && btc_momentum > cfg.momentum_threshold);

        if momentum_opposes {
            return Decision::Pass(format!(
                "momentum_opposite_{:+.1}%",
                (btc_momentum * decimal("100")).round_dp(1)
            ));
        }
    }

    // 9. Dynamic fair value adjustment based on volatility
    // High volatility = more uncertainty = higher fair_value = more edge required
    let effective_fair_value = if cfg.dynamic_fv_enabled && btc_volatility > Decimal::ZERO {
        let boost = btc_volatility * cfg.volatility_weight;
        cfg.fair_value + boost
    } else {
        cfg.fair_value
    };

    // 10. Calculate edge with effective fair value
    let edge = effective_fair_value - cheap_price;

    // Edge must be positive
    if edge <= Decimal::ZERO {
        return Decision::Pass(format!(
            "no_edge_{:.0}%",
            (edge * decimal("100")).round_dp(0)
        ));
    }

    // 11. Calculate payoff ratio
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
        btc_momentum,
        btc_volatility,
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
            btc_prices: vec![
                PriceTick {
                    price: d("70000"),
                    timestamp_ms: 1000,
                },
                PriceTick {
                    price: d("70100"),
                    timestamp_ms: 2000,
                },
                PriceTick {
                    price: d("70200"),
                    timestamp_ms: 3000,
                },
            ],
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
    fn test_circuit_breaker_on_consecutive_losses() {
        let mut account = AccountState::new(d("1000"));
        account.consecutive_losses = 5;
        let cfg = DeciderConfig {
            max_consecutive_losses: 5,
            ..DeciderConfig::default()
        };
        let ctx = default_ctx();

        let decision = decide(&ctx, &account, &cfg);

        match decision {
            Decision::Pass(reason) => assert!(reason.starts_with("circuit_breaker")),
            Decision::Trade { .. } => panic!("expected pass but got trade"),
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
    fn test_momentum_filter_rejects_opposite_direction() {
        let account = AccountState::new(d("1000"));
        let cfg = DeciderConfig {
            momentum_filter_enabled: true,
            momentum_threshold: d("0.001"),
            ..DeciderConfig::default()
        };

        // Strong upward momentum should reject DOWN trade
        let mut ctx = default_ctx();
        ctx.btc_prices = vec![
            PriceTick {
                price: d("70000"),
                timestamp_ms: 1000,
            },
            PriceTick {
                price: d("70500"),
                timestamp_ms: 61000,
            }, // +0.7% in 60s
        ];

        let decision = decide(&ctx, &account, &cfg);

        match decision {
            Decision::Pass(reason) => assert!(reason.starts_with("momentum_opposite")),
            Decision::Trade { .. } => panic!("expected pass due to momentum filter"),
        }
    }

    #[test]
    fn test_compute_momentum() {
        let prices = vec![
            PriceTick {
                price: d("100"),
                timestamp_ms: 0,
            },
            PriceTick {
                price: d("101"),
                timestamp_ms: 30000,
            },
            PriceTick {
                price: d("102"),
                timestamp_ms: 60000,
            },
        ];

        let momentum = compute_momentum(&prices, 60);
        assert_eq!(momentum, d("0.02")); // 2% gain
    }

    #[test]
    fn test_compute_volatility() {
        let prices = vec![
            PriceTick {
                price: d("100"),
                timestamp_ms: 0,
            },
            PriceTick {
                price: d("101"),
                timestamp_ms: 1000,
            },
            PriceTick {
                price: d("99"),
                timestamp_ms: 2000,
            },
            PriceTick {
                price: d("100"),
                timestamp_ms: 3000,
            },
        ];

        let volatility = compute_volatility(&prices, 60);
        assert!(volatility > Decimal::ZERO);
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
}
