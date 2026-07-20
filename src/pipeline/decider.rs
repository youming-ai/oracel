//! Stage 2: oracle-aligned value and momentum trade decider.
//!
//! Chainlink defines the market outcome. Binance confirms short-term momentum,
//! while executable CLOB order-book prices determine whether an entry has
//! enough conservative net edge.

use std::collections::VecDeque;

use rust_decimal::{Decimal, MathematicalOps};

use crate::data::polymarket::BuyQuote;
use crate::util;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Direction {
    Up,
    Down,
}

impl Direction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Direction::Up => "UP",
            Direction::Down => "DOWN",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Decision {
    Pass(String),
    Trade {
        direction: Direction,
        size_usdc: Decimal,
        /// Conservative edge after fee and model-uncertainty buffers.
        edge: Decimal,
        payoff_ratio: Decimal,
        model_probability: Decimal,
        entry_price: Decimal,
        /// Highest price that still preserves the configured minimum edge.
        order_limit_price: Decimal,
    },
}

#[derive(Debug, Clone)]
pub struct DeciderConfig {
    pub position_size_usdc: Decimal,
    pub min_entry_ttl_ms: u64,
    pub max_entry_ttl_ms: u64,
    pub min_normalized_move: Decimal,
    pub min_net_edge: Decimal,
    pub model_uncertainty: Decimal,
    pub fee_buffer: Decimal,
    pub max_spread: Decimal,
    pub min_depth_multiple: Decimal,
    pub daily_loss_limit_usdc: Decimal,
    pub max_consecutive_losses: u32,
    pub loss_cooldown_ms: i64,
    pub max_trades_per_day: u32,
    pub circuit_breaker_window: u32,
    pub circuit_breaker_min_win_rate: Decimal,
}

impl Default for DeciderConfig {
    fn default() -> Self {
        Self {
            position_size_usdc: util::decimal("1"),
            min_entry_ttl_ms: 75_000,
            max_entry_ttl_ms: 150_000,
            min_normalized_move: util::decimal("0.60"),
            min_net_edge: util::decimal("0.05"),
            model_uncertainty: util::decimal("0.03"),
            fee_buffer: util::decimal("0.02"),
            max_spread: util::decimal("0.03"),
            min_depth_multiple: util::decimal("5"),
            daily_loss_limit_usdc: Decimal::ZERO,
            max_consecutive_losses: 3,
            loss_cooldown_ms: 1_800_000,
            max_trades_per_day: 8,
            circuit_breaker_window: 50,
            circuit_breaker_min_win_rate: util::decimal("0.05"),
        }
    }
}

impl From<&crate::config::Config> for DeciderConfig {
    fn from(cfg: &crate::config::Config) -> Self {
        Self {
            position_size_usdc: cfg.strategy.position_size_usdc,
            min_entry_ttl_ms: cfg.strategy.min_entry_ttl_ms,
            max_entry_ttl_ms: cfg.strategy.max_entry_ttl_ms,
            min_normalized_move: cfg.strategy.min_normalized_move,
            min_net_edge: cfg.strategy.min_net_edge,
            model_uncertainty: cfg.strategy.model_uncertainty,
            fee_buffer: cfg.strategy.fee_buffer,
            max_spread: cfg.strategy.max_spread,
            min_depth_multiple: cfg.strategy.min_depth_multiple,
            daily_loss_limit_usdc: cfg.risk.daily_loss_limit_usdc,
            max_consecutive_losses: cfg.risk.max_consecutive_losses,
            loss_cooldown_ms: cfg.risk.loss_cooldown_ms,
            max_trades_per_day: cfg.risk.max_trades_per_day,
            circuit_breaker_window: cfg.strategy.circuit_breaker_window,
            circuit_breaker_min_win_rate: cfg.strategy.circuit_breaker_min_win_rate,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AccountState {
    pub balance: Decimal,
    pub initial_balance: Decimal,
    pub consecutive_losses: u32,
    pub consecutive_wins: u32,
    pub total_wins: u32,
    pub total_losses: u32,
    pub daily_pnl: Decimal,
    pub daily_trades: u32,
    pub last_loss_ms: i64,
    pub daily_reset_date: String,
    pub recent_results: VecDeque<bool>,
}

impl AccountState {
    pub fn new(balance: Decimal) -> Self {
        Self {
            balance,
            initial_balance: balance,
            consecutive_losses: 0,
            consecutive_wins: 0,
            total_wins: 0,
            total_losses: 0,
            daily_pnl: Decimal::ZERO,
            daily_trades: 0,
            last_loss_ms: 0,
            daily_reset_date: String::new(),
            recent_results: VecDeque::new(),
        }
    }

    pub fn pnl(&self) -> Decimal {
        self.balance - self.initial_balance
    }

    pub fn record_trade(&mut self, cost: Decimal) {
        self.balance -= cost;
        self.daily_trades = self.daily_trades.saturating_add(1);
    }

    pub fn record_settlement(&mut self, result: &crate::pipeline::settler::SettlementResult) {
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
            self.last_loss_ms = chrono::Utc::now().timestamp_millis();
        }
        self.recent_results.push_back(result.won);
        if self.recent_results.len() > 200 {
            self.recent_results.pop_front();
        }
    }

    pub fn reset_daily_if_needed(&mut self, today: &str) {
        if self.daily_reset_date != today {
            self.daily_pnl = Decimal::ZERO;
            self.daily_trades = 0;
            self.daily_reset_date = today.to_string();
        }
    }
}

pub struct DecideContext {
    pub now_ms: i64,
    pub up_quote: Option<BuyQuote>,
    pub down_quote: Option<BuyQuote>,
    pub remaining_ms: i64,
    pub chainlink_open: Option<Decimal>,
    pub chainlink_current: Option<Decimal>,
    pub sigma_per_second: Option<Decimal>,
    pub binance_trend_15s_pct: Option<Decimal>,
    pub binance_trend_30s_pct: Option<Decimal>,
}

fn integer_suffix(value: Decimal) -> String {
    value.abs().trunc().to_string()
}

/// Decimal-only approximation of the standard normal cumulative distribution.
fn normal_cdf(value: Decimal) -> Decimal {
    let six = Decimal::from(6);
    if value >= six {
        return util::decimal("0.999999");
    }
    if value <= -six {
        return util::decimal("0.000001");
    }

    let x = value.abs();
    let t = Decimal::ONE / (Decimal::ONE + util::decimal("0.2316419") * x);
    let polynomial = t
        * (util::decimal("0.319381530")
            + t * (util::decimal("-0.356563782")
                + t * (util::decimal("1.781477937")
                    + t * (util::decimal("-1.821255978") + t * util::decimal("1.330274429")))));
    let density = (-x * x / Decimal::from(2)).exp() * util::decimal("0.3989422804014327");
    let positive_cdf = Decimal::ONE - density * polynomial;
    if value >= Decimal::ZERO {
        positive_cdf
    } else {
        Decimal::ONE - positive_cdf
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelEstimate {
    pub normalized_move: Decimal,
    pub p_up: Decimal,
}

pub fn estimate_probability(ctx: &DecideContext) -> Option<ModelEstimate> {
    let open = ctx.chainlink_open?;
    let current = ctx.chainlink_current?;
    let sigma_per_second = ctx.sigma_per_second?;
    if open <= Decimal::ZERO || current <= Decimal::ZERO || sigma_per_second <= Decimal::ZERO {
        return None;
    }
    let ttl_seconds = Decimal::from(ctx.remaining_ms) / Decimal::from(1_000);
    let expected_remaining_volatility = sigma_per_second * ttl_seconds.sqrt()?;
    if expected_remaining_volatility <= Decimal::ZERO {
        return None;
    }
    let distance_return = (current - open) / open;
    let normalized_move = distance_return / expected_remaining_volatility;
    Some(ModelEstimate {
        normalized_move,
        p_up: normal_cdf(normalized_move),
    })
}

fn circuit_breaker_reason(account: &AccountState, cfg: &DeciderConfig) -> Option<String> {
    let window = &account.recent_results;
    if cfg.circuit_breaker_window == 0 || window.len() < cfg.circuit_breaker_window as usize {
        return None;
    }
    let wins = window.iter().filter(|&&won| won).count() as u32;
    let win_rate = Decimal::from(wins) / Decimal::from(window.len() as u32);
    (win_rate < cfg.circuit_breaker_min_win_rate).then(|| {
        format!(
            "circuit_breaker_wr_{}%",
            (win_rate * Decimal::from(100)).round_dp(0)
        )
    })
}

pub fn decide(ctx: &DecideContext, account: &AccountState, cfg: &DeciderConfig) -> Decision {
    if account.balance < cfg.position_size_usdc {
        return Decision::Pass("insufficient_balance".into());
    }
    if cfg.daily_loss_limit_usdc > Decimal::ZERO && account.daily_pnl <= -cfg.daily_loss_limit_usdc
    {
        return Decision::Pass(format!(
            "daily_loss_limit_{}",
            integer_suffix(account.daily_pnl)
        ));
    }
    if account.consecutive_losses >= cfg.max_consecutive_losses
        && ctx.now_ms.saturating_sub(account.last_loss_ms) < cfg.loss_cooldown_ms
    {
        return Decision::Pass(format!(
            "consecutive_loss_cooldown_{}",
            account.consecutive_losses
        ));
    }
    if account.daily_trades >= cfg.max_trades_per_day {
        return Decision::Pass(format!("daily_trade_limit_{}", account.daily_trades));
    }
    if let Some(reason) = circuit_breaker_reason(account, cfg) {
        return Decision::Pass(reason);
    }

    let min_ttl = i64::try_from(cfg.min_entry_ttl_ms).unwrap_or(i64::MAX);
    let max_ttl = i64::try_from(cfg.max_entry_ttl_ms).unwrap_or(i64::MAX);
    if ctx.remaining_ms < min_ttl {
        return Decision::Pass(format!(
            "ttl_below_entry_floor_{}",
            ctx.remaining_ms.max(0) / 1_000
        ));
    }
    if ctx.remaining_ms > max_ttl {
        return Decision::Pass(format!(
            "ttl_above_entry_ceiling_{}",
            ctx.remaining_ms / 1_000
        ));
    }

    let Some(estimate) = estimate_probability(ctx) else {
        return Decision::Pass("oracle_model_not_ready".into());
    };
    let z = estimate.normalized_move;
    if z.abs() < cfg.min_normalized_move {
        return Decision::Pass(format!("normalized_move_{:.2}", z.abs()));
    }

    let (direction, model_probability, quote) = if z > Decimal::ZERO {
        let momentum_confirmed = ctx
            .binance_trend_15s_pct
            .is_some_and(|trend| trend > Decimal::ZERO)
            && ctx
                .binance_trend_30s_pct
                .is_some_and(|trend| trend > Decimal::ZERO);
        if !momentum_confirmed {
            return Decision::Pass("binance_momentum_not_confirmed_up".into());
        }
        (Direction::Up, estimate.p_up, ctx.up_quote)
    } else {
        let momentum_confirmed = ctx
            .binance_trend_15s_pct
            .is_some_and(|trend| trend < Decimal::ZERO)
            && ctx
                .binance_trend_30s_pct
                .is_some_and(|trend| trend < Decimal::ZERO);
        if !momentum_confirmed {
            return Decision::Pass("binance_momentum_not_confirmed_down".into());
        }
        (
            Direction::Down,
            Decimal::ONE - estimate.p_up,
            ctx.down_quote,
        )
    };
    let Some(quote) = quote else {
        return Decision::Pass("no_order_book_quote".into());
    };

    let Some(spread) = quote.spread else {
        return Decision::Pass("no_order_book_bid".into());
    };
    if spread > cfg.max_spread {
        return Decision::Pass(format!("spread_too_wide_{:.3}", spread));
    }
    let required_depth = cfg.position_size_usdc * cfg.min_depth_multiple;
    if quote.best_ask_notional < required_depth {
        return Decision::Pass(format!(
            "insufficient_top_ask_depth_{:.2}",
            quote.best_ask_notional
        ));
    }

    let conservative_edge =
        model_probability - quote.effective_price - cfg.model_uncertainty - cfg.fee_buffer;
    if conservative_edge < cfg.min_net_edge {
        return Decision::Pass(format!("net_edge_{:.3}", conservative_edge));
    }
    let order_limit_price =
        model_probability - cfg.model_uncertainty - cfg.fee_buffer - cfg.min_net_edge;
    if quote.limit_price > order_limit_price {
        return Decision::Pass(format!("book_limit_exceeds_value_{:.3}", quote.limit_price));
    }

    Decision::Trade {
        direction,
        size_usdc: cfg.position_size_usdc,
        edge: conservative_edge,
        payoff_ratio: (Decimal::ONE - quote.effective_price) / quote.effective_price,
        model_probability,
        entry_price: quote.effective_price,
        order_limit_price,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::test_helpers::d;

    fn quote(price: &str) -> BuyQuote {
        let price = d(price);
        BuyQuote {
            best_bid: Some(price - d("0.01")),
            best_ask: price,
            spread: Some(d("0.01")),
            best_ask_notional: d("20"),
            effective_price: price,
            limit_price: price,
            timestamp_ms: 1_700_000_000_000,
        }
    }

    fn context() -> DecideContext {
        DecideContext {
            now_ms: 1_700_000_000_000,
            up_quote: Some(quote("0.60")),
            down_quote: Some(quote("0.40")),
            remaining_ms: 120_000,
            chainlink_open: Some(d("64000")),
            chainlink_current: Some(d("64100")),
            sigma_per_second: Some(d("0.0001")),
            binance_trend_15s_pct: Some(d("0.10")),
            binance_trend_30s_pct: Some(d("0.15")),
        }
    }

    #[test]
    fn normal_cdf_is_symmetric_and_monotonic() {
        assert!((normal_cdf(Decimal::ZERO) - d("0.5")).abs() < d("0.000001"));
        assert!(normal_cdf(d("1")) > normal_cdf(d("0.5")));
        assert!((normal_cdf(d("1")) + normal_cdf(d("-1")) - Decimal::ONE).abs() < d("0.000001"));
    }

    #[test]
    fn trades_up_when_oracle_value_and_momentum_agree() {
        let decision = decide(
            &context(),
            &AccountState::new(d("100")),
            &DeciderConfig::default(),
        );
        assert!(matches!(
            decision,
            Decision::Trade {
                direction: Direction::Up,
                ..
            }
        ));
    }

    #[test]
    fn trades_down_when_oracle_value_and_momentum_agree() {
        let mut ctx = context();
        ctx.chainlink_current = Some(d("63900"));
        ctx.binance_trend_15s_pct = Some(d("-0.10"));
        ctx.binance_trend_30s_pct = Some(d("-0.15"));
        ctx.down_quote = Some(quote("0.60"));
        assert!(matches!(
            decide(
                &ctx,
                &AccountState::new(d("100")),
                &DeciderConfig::default()
            ),
            Decision::Trade {
                direction: Direction::Down,
                ..
            }
        ));
    }

    #[test]
    fn rejects_entries_outside_ttl_window() {
        let mut ctx = context();
        ctx.remaining_ms = 151_000;
        assert!(matches!(
            decide(&ctx, &AccountState::new(d("100")), &DeciderConfig::default()),
            Decision::Pass(reason) if reason.starts_with("ttl_above_entry_ceiling")
        ));
        ctx.remaining_ms = 74_000;
        assert!(matches!(
            decide(&ctx, &AccountState::new(d("100")), &DeciderConfig::default()),
            Decision::Pass(reason) if reason.starts_with("ttl_below_entry_floor")
        ));
    }

    #[test]
    fn rejects_momentum_disagreement() {
        let mut ctx = context();
        ctx.binance_trend_15s_pct = Some(d("-0.01"));
        assert!(matches!(
            decide(&ctx, &AccountState::new(d("100")), &DeciderConfig::default()),
            Decision::Pass(reason) if reason == "binance_momentum_not_confirmed_up"
        ));
    }

    #[test]
    fn rejects_wide_spread_and_thin_depth() {
        let mut ctx = context();
        let mut wide = quote("0.60");
        wide.spread = Some(d("0.04"));
        ctx.up_quote = Some(wide);
        assert!(matches!(
            decide(&ctx, &AccountState::new(d("100")), &DeciderConfig::default()),
            Decision::Pass(reason) if reason.starts_with("spread_too_wide")
        ));

        let mut thin = quote("0.60");
        thin.best_ask_notional = d("4.99");
        ctx.up_quote = Some(thin);
        assert!(matches!(
            decide(&ctx, &AccountState::new(d("100")), &DeciderConfig::default()),
            Decision::Pass(reason) if reason.starts_with("insufficient_top_ask_depth")
        ));
    }

    #[test]
    fn rejects_unprofitable_quote() {
        let mut ctx = context();
        ctx.up_quote = Some(quote("0.90"));
        assert!(matches!(
            decide(&ctx, &AccountState::new(d("100")), &DeciderConfig::default()),
            Decision::Pass(reason) if reason.starts_with("net_edge")
        ));
    }

    #[test]
    fn risk_controls_still_apply() {
        let mut account = AccountState::new(d("100"));
        account.daily_pnl = d("-5");
        let cfg = DeciderConfig {
            daily_loss_limit_usdc: d("5"),
            ..DeciderConfig::default()
        };
        assert!(matches!(
            decide(&context(), &account, &cfg),
            Decision::Pass(reason) if reason == "daily_loss_limit_5"
        ));
    }

    #[test]
    fn consecutive_loss_and_daily_trade_limits_apply() {
        let mut account = AccountState::new(d("100"));
        account.consecutive_losses = 3;
        account.last_loss_ms = context().now_ms - 1_000;
        assert!(matches!(
            decide(&context(), &account, &DeciderConfig::default()),
            Decision::Pass(reason) if reason == "consecutive_loss_cooldown_3"
        ));

        account.consecutive_losses = 0;
        account.daily_trades = 8;
        assert!(matches!(
            decide(&context(), &account, &DeciderConfig::default()),
            Decision::Pass(reason) if reason == "daily_trade_limit_8"
        ));
    }

    #[test]
    fn settlement_accounting_remains_exact() {
        let mut account = AccountState::new(d("100"));
        account.record_trade(d("1"));
        account.record_settlement(&crate::pipeline::settler::SettlementResult {
            direction: Direction::Up,
            payout: d("1.5"),
            pnl: d("0.5"),
            won: true,
            condition_id: "cid".into(),
            market_slug: "btc-updown-5m-test".into(),
            entry_btc_price: d("64000"),
        });
        assert_eq!(account.balance, d("100.5"));
        assert_eq!(account.daily_pnl, d("0.5"));
    }
}
