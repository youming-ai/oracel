//! Oracle-free Binance value-momentum decision engine.
//!
//! Binance Prediction supplies the official window opening price and order book.
//! Binance BTCUSDT supplies the current price, realized volatility, and momentum.

use std::collections::VecDeque;

use rust_decimal::{Decimal, MathematicalOps};

use crate::config::Config;
use crate::data::binance_prediction::BuyQuote;
use crate::util;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Direction {
    Up,
    Down,
}

impl Direction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Up => "UP",
            Self::Down => "DOWN",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Decision {
    Pass(String),
    Trade {
        direction: Direction,
        size_usdt: Decimal,
        /// Conservative probability edge after model and fee buffers.
        edge: Decimal,
        payoff_ratio: Decimal,
        model_probability: Decimal,
        entry_price: Decimal,
        /// The highest price that preserves the required edge.
        max_price: Decimal,
    },
}

#[derive(Debug, Clone)]
pub struct DeciderConfig {
    pub position_size_usdt: Decimal,
    pub min_entry_ttl_ms: u64,
    pub max_entry_ttl_ms: u64,
    pub min_normalized_move: Decimal,
    pub min_net_edge: Decimal,
    pub model_uncertainty: Decimal,
    pub fee_buffer: Decimal,
    pub max_spread: Decimal,
    pub min_depth_multiple: Decimal,
    pub daily_loss_limit_usdt: Decimal,
    pub max_consecutive_losses: u32,
    pub loss_cooldown_ms: i64,
    pub max_trades_per_day: u32,
    pub circuit_breaker_window: u32,
    pub circuit_breaker_min_win_rate: Decimal,
}

impl From<&Config> for DeciderConfig {
    fn from(config: &Config) -> Self {
        Self {
            position_size_usdt: config.strategy.position_size_usdt,
            min_entry_ttl_ms: config.strategy.min_entry_ttl_ms,
            max_entry_ttl_ms: config.strategy.max_entry_ttl_ms,
            min_normalized_move: config.strategy.min_normalized_move,
            min_net_edge: config.strategy.min_net_edge,
            model_uncertainty: config.strategy.model_uncertainty,
            fee_buffer: config.strategy.fee_buffer,
            max_spread: config.strategy.max_spread,
            min_depth_multiple: config.strategy.min_depth_multiple,
            daily_loss_limit_usdt: config.risk.daily_loss_limit_usdt,
            max_consecutive_losses: config.risk.max_consecutive_losses,
            loss_cooldown_ms: config.risk.loss_cooldown_ms,
            max_trades_per_day: config.risk.max_trades_per_day,
            circuit_breaker_window: config.strategy.circuit_breaker_window,
            circuit_breaker_min_win_rate: config.strategy.circuit_breaker_min_win_rate,
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

    pub fn record_settlement(
        &mut self,
        won: bool,
        payout: Decimal,
        pnl: Decimal,
        apply_payout_to_balance: bool,
        now_ms: i64,
    ) {
        if apply_payout_to_balance {
            self.balance += payout;
        }
        self.daily_pnl += pnl;
        if won {
            self.consecutive_wins += 1;
            self.consecutive_losses = 0;
            self.total_wins += 1;
        } else {
            self.consecutive_losses += 1;
            self.consecutive_wins = 0;
            self.total_losses += 1;
            self.last_loss_ms = now_ms;
        }
        self.recent_results.push_back(won);
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
    /// Binance Prediction market fee advertised by the active market.
    pub market_fee_rate_bps: Option<u32>,
    pub up_quote: Option<BuyQuote>,
    pub down_quote: Option<BuyQuote>,
    pub remaining_ms: i64,
    /// Official Binance Prediction window starting price.
    pub reference_price: Option<Decimal>,
    /// Latest Binance BTCUSDT spot price.
    pub current_price: Option<Decimal>,
    pub sigma_per_second: Option<Decimal>,
    pub binance_trend_15s_pct: Option<Decimal>,
    pub binance_trend_30s_pct: Option<Decimal>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelEstimate {
    pub normalized_move: Decimal,
    pub p_up: Decimal,
}

/// Decimal-only approximation of the standard normal CDF.
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
    let positive = Decimal::ONE - density * polynomial;
    if value >= Decimal::ZERO {
        positive
    } else {
        Decimal::ONE - positive
    }
}

pub fn estimate_probability(context: &DecideContext) -> Option<ModelEstimate> {
    let reference = context.reference_price?;
    let current = context.current_price?;
    let sigma = context.sigma_per_second?;
    if reference <= Decimal::ZERO || current <= Decimal::ZERO || sigma <= Decimal::ZERO {
        return None;
    }
    let ttl_seconds = Decimal::from(context.remaining_ms) / Decimal::from(1_000);
    let remaining_volatility = sigma * ttl_seconds.sqrt()?;
    if remaining_volatility <= Decimal::ZERO {
        return None;
    }
    let normalized_move = ((current - reference) / reference) / remaining_volatility;
    Some(ModelEstimate {
        normalized_move,
        p_up: normal_cdf(normalized_move),
    })
}

pub fn decide(context: &DecideContext, account: &AccountState, config: &DeciderConfig) -> Decision {
    let fee_rate_bps = context.market_fee_rate_bps.unwrap_or_default();
    let required_cash = config.position_size_usdt
        * (Decimal::ONE + Decimal::from(fee_rate_bps) / Decimal::from(10_000));
    if account.balance < required_cash {
        return Decision::Pass("insufficient_balance".into());
    }
    if config.daily_loss_limit_usdt > Decimal::ZERO
        && account.daily_pnl <= -config.daily_loss_limit_usdt
    {
        return Decision::Pass(format!(
            "daily_loss_limit_{}",
            account.daily_pnl.abs().trunc()
        ));
    }
    if account.consecutive_losses >= config.max_consecutive_losses
        && context.now_ms.saturating_sub(account.last_loss_ms) < config.loss_cooldown_ms
    {
        return Decision::Pass(format!(
            "consecutive_loss_cooldown_{}",
            account.consecutive_losses
        ));
    }
    if account.daily_trades >= config.max_trades_per_day {
        return Decision::Pass(format!("daily_trade_limit_{}", account.daily_trades));
    }
    if config.circuit_breaker_window > 0
        && account.recent_results.len() >= config.circuit_breaker_window as usize
    {
        let wins = account.recent_results.iter().filter(|&&won| won).count();
        let rate = Decimal::from(wins as u32) / Decimal::from(account.recent_results.len() as u32);
        if rate < config.circuit_breaker_min_win_rate {
            return Decision::Pass(format!(
                "circuit_breaker_wr_{}%",
                (rate * Decimal::from(100)).round_dp(0)
            ));
        }
    }

    let min_ttl = i64::try_from(config.min_entry_ttl_ms).unwrap_or(i64::MAX);
    let max_ttl = i64::try_from(config.max_entry_ttl_ms).unwrap_or(i64::MAX);
    if context.remaining_ms < min_ttl {
        return Decision::Pass(format!(
            "ttl_below_entry_floor_{}",
            context.remaining_ms.max(0) / 1_000
        ));
    }
    if context.remaining_ms > max_ttl {
        return Decision::Pass(format!(
            "ttl_above_entry_ceiling_{}",
            context.remaining_ms / 1_000
        ));
    }

    let Some(estimate) = estimate_probability(context) else {
        return Decision::Pass("binance_model_not_ready".into());
    };
    if estimate.normalized_move.abs() < config.min_normalized_move {
        return Decision::Pass(format!(
            "normalized_move_{:.2}",
            estimate.normalized_move.abs()
        ));
    }

    let (direction, probability, quote) = if estimate.normalized_move > Decimal::ZERO {
        if !context
            .binance_trend_15s_pct
            .is_some_and(|value| value > Decimal::ZERO)
            || !context
                .binance_trend_30s_pct
                .is_some_and(|value| value > Decimal::ZERO)
        {
            return Decision::Pass("binance_momentum_not_confirmed_up".into());
        }
        (Direction::Up, estimate.p_up, context.up_quote)
    } else {
        if !context
            .binance_trend_15s_pct
            .is_some_and(|value| value < Decimal::ZERO)
            || !context
                .binance_trend_30s_pct
                .is_some_and(|value| value < Decimal::ZERO)
        {
            return Decision::Pass("binance_momentum_not_confirmed_down".into());
        }
        (
            Direction::Down,
            Decimal::ONE - estimate.p_up,
            context.down_quote,
        )
    };
    let Some(quote) = quote else {
        return Decision::Pass("no_binance_prediction_quote".into());
    };
    let Some(spread) = quote.spread else {
        return Decision::Pass("no_binance_prediction_bid".into());
    };
    if spread > config.max_spread {
        return Decision::Pass(format!("spread_too_wide_{spread:.3}"));
    }
    let required_depth = config.position_size_usdt * config.min_depth_multiple;
    if quote.best_ask_notional < required_depth {
        return Decision::Pass(format!(
            "insufficient_top_ask_depth_{:.2}",
            quote.best_ask_notional
        ));
    }

    let edge = probability - quote.effective_price - config.model_uncertainty - config.fee_buffer;
    if edge < config.min_net_edge {
        return Decision::Pass(format!("net_edge_{edge:.3}"));
    }
    let max_price =
        probability - config.model_uncertainty - config.fee_buffer - config.min_net_edge;
    if quote.limit_price > max_price {
        return Decision::Pass(format!("book_limit_exceeds_value_{:.3}", quote.limit_price));
    }

    Decision::Trade {
        direction,
        size_usdt: config.position_size_usdt,
        edge,
        payoff_ratio: (Decimal::ONE - quote.effective_price) / quote.effective_price,
        model_probability: probability,
        entry_price: quote.effective_price,
        max_price,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::test_helpers::d;

    fn config() -> DeciderConfig {
        DeciderConfig {
            position_size_usdt: d("2"),
            min_entry_ttl_ms: 75_000,
            max_entry_ttl_ms: 150_000,
            min_normalized_move: d("0.60"),
            min_net_edge: d("0.05"),
            model_uncertainty: d("0.03"),
            fee_buffer: d("0.02"),
            max_spread: d("0.03"),
            min_depth_multiple: d("5"),
            daily_loss_limit_usdt: d("5"),
            max_consecutive_losses: 3,
            loss_cooldown_ms: 1_800_000,
            max_trades_per_day: 8,
            circuit_breaker_window: 50,
            circuit_breaker_min_win_rate: d("0.05"),
        }
    }

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
            market_fee_rate_bps: Some(200),
            up_quote: Some(quote("0.60")),
            down_quote: Some(quote("0.40")),
            remaining_ms: 120_000,
            reference_price: Some(d("64000")),
            current_price: Some(d("64100")),
            sigma_per_second: Some(d("0.0001")),
            binance_trend_15s_pct: Some(d("0.10")),
            binance_trend_30s_pct: Some(d("0.15")),
        }
    }

    #[test]
    fn normal_probability_is_symmetric() {
        assert!((normal_cdf(Decimal::ZERO) - d("0.5")).abs() < d("0.000001"));
        assert!((normal_cdf(d("1")) + normal_cdf(d("-1")) - Decimal::ONE).abs() < d("0.000001"));
    }

    #[test]
    fn trades_up_when_binance_reference_and_momentum_agree() {
        assert!(matches!(
            decide(&context(), &AccountState::new(d("100")), &config()),
            Decision::Trade {
                direction: Direction::Up,
                ..
            }
        ));
    }

    #[test]
    fn trades_down_when_binance_reference_and_momentum_agree() {
        let mut context = context();
        context.current_price = Some(d("63900"));
        context.binance_trend_15s_pct = Some(d("-0.10"));
        context.binance_trend_30s_pct = Some(d("-0.15"));
        context.down_quote = Some(quote("0.60"));
        assert!(matches!(
            decide(&context, &AccountState::new(d("100")), &config()),
            Decision::Trade {
                direction: Direction::Down,
                ..
            }
        ));
    }

    #[test]
    fn rejects_ttl_momentum_and_liquidity_failures() {
        let mut ctx = context();
        ctx.remaining_ms = 151_000;
        assert!(matches!(
            decide(&ctx, &AccountState::new(d("100")), &config()),
            Decision::Pass(reason) if reason.starts_with("ttl_above")
        ));

        ctx = context();
        ctx.binance_trend_15s_pct = Some(d("-0.01"));
        assert!(matches!(
            decide(&ctx, &AccountState::new(d("100")), &config()),
            Decision::Pass(reason) if reason == "binance_momentum_not_confirmed_up"
        ));

        ctx = context();
        let mut thin = quote("0.60");
        thin.best_ask_notional = d("9.99");
        ctx.up_quote = Some(thin);
        assert!(matches!(
            decide(&ctx, &AccountState::new(d("100")), &config()),
            Decision::Pass(reason) if reason.starts_with("insufficient_top_ask_depth")
        ));
    }

    #[test]
    fn applies_daily_and_loss_streak_risk_limits() {
        let mut account = AccountState::new(d("100"));
        account.daily_pnl = d("-5");
        assert!(matches!(
            decide(&context(), &account, &config()),
            Decision::Pass(reason) if reason == "daily_loss_limit_5"
        ));

        account.daily_pnl = Decimal::ZERO;
        account.consecutive_losses = 3;
        account.last_loss_ms = context().now_ms - 1_000;
        assert!(matches!(
            decide(&context(), &account, &config()),
            Decision::Pass(reason) if reason == "consecutive_loss_cooldown_3"
        ));
    }

    #[test]
    fn settlement_accounting_applies_exact_decimal_pnl() {
        let mut account = AccountState::new(d("100"));
        account.record_trade(d("2"));
        account.record_settlement(true, d("3.5"), d("1.5"), true, 1_700_000_000_000);
        assert_eq!(account.balance, d("101.5"));
        assert_eq!(account.daily_pnl, d("1.5"));
    }
}
