//! Bot configuration — loaded from `config.toml`.

use rust_decimal::Decimal;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

fn dec(s: &'static str) -> Decimal {
    Decimal::from_str_exact(s).expect(s)
}

pub(crate) mod defaults {
    use super::*;

    // ─── Trading ───
    pub fn paper_starting_balance() -> Decimal {
        dec("100")
    }

    // ─── Market ───
    pub fn stale_threshold_ms() -> i64 {
        30_000
    }
    pub fn min_ttl_ms() -> i64 {
        30_000
    }

    // ─── Polymarket CLOB ───
    pub fn gamma_api_url() -> String {
        "https://gamma-api.polymarket.com".to_string()
    }

    // ─── Strategy ───
    pub fn extreme_threshold() -> Decimal {
        dec("0.90")
    }
    pub fn fair_value() -> Decimal {
        dec("0.50")
    }
    pub fn position_size_usdc() -> Decimal {
        dec("1.0")
    }
    pub fn min_entry_price() -> Decimal {
        dec("0.02")
    }
    pub fn max_entry_price() -> Decimal {
        dec("0.12")
    }
    pub fn min_ttl_for_entry_ms() -> u64 {
        120_000
    }
    pub fn btc_trend_window_s() -> u64 {
        30
    }
    pub fn btc_trend_min_pct() -> Decimal {
        dec("0.05")
    }
    pub fn circuit_breaker_window() -> u32 {
        50
    }
    pub fn circuit_breaker_min_win_rate() -> Decimal {
        dec("0.05")
    }

    // ─── Risk ───
    pub fn daily_loss_limit() -> Decimal {
        dec("0")
    }
    pub fn max_fak_retries() -> u32 {
        3
    }
    pub fn fak_backoff_ms() -> u64 {
        3_000
    }

    // ─── Polling ───
    pub fn signal_interval_ms() -> u64 {
        1_000
    }
    pub fn status_interval_ms() -> u64 {
        10_000
    }
    pub fn market_refresh_secs() -> u64 {
        60
    }
    pub fn settlement_check_secs() -> u64 {
        15
    }

    // ─── Execution ───
    pub fn slippage_tolerance() -> Decimal {
        dec("0.01")
    }

    // ─── Price Source ───
    pub fn symbol() -> String {
        "BTCUSDT".to_string()
    }
    pub fn price_buffer_max() -> usize {
        1000
    }
    pub fn price_buffer_min_ticks() -> usize {
        60
    }

    // ─── Timeouts ───
    pub fn gamma_http_timeout_secs() -> u64 {
        10
    }
    pub fn ws_connect_timeout_secs() -> u64 {
        10
    }
    pub fn ws_max_backoff_secs() -> u64 {
        60
    }
    pub fn clob_price_timeout_secs() -> u64 {
        10
    }
    pub fn clob_auth_timeout_secs() -> u64 {
        15
    }
    pub fn clob_order_timeout_secs() -> u64 {
        15
    }
    pub fn rpc_connect_timeout_secs() -> u64 {
        30
    }
    pub fn rpc_redeem_timeout_secs() -> u64 {
        30
    }
    pub fn balance_query_timeout_secs() -> u64 {
        10
    }

    // ─── Redeem ───
    pub fn redeem_max_retries() -> u32 {
        10
    }
    pub fn redeem_delay_secs() -> u64 {
        5
    }
    pub fn redeem_concurrency() -> usize {
        5
    }

    // ─── Misc ───
    pub fn trade_log_flush_secs() -> u64 {
        30
    }
    pub fn shutdown_timeout_secs() -> u64 {
        5
    }
    pub fn market_search_windows() -> u32 {
        5
    }
    pub fn resolution_price_threshold() -> f64 {
        0.999
    }

    // ─── Time Windows ───
    pub fn window1_start() -> u32 {
        0
    }
    pub fn window1_end() -> u32 {
        12
    }
    pub fn window2_start() -> u32 {
        12
    }
    pub fn window2_end() -> u32 {
        24
    }

    pub fn private_key() -> SecretString {
        SecretString::new(String::new().into())
    }
}

// ─── Top-level Config ───

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub trading: TradingConfig,
    #[serde(default)]
    pub market: MarketConfig,
    #[serde(default)]
    pub polyclob: PolymarketConfig,
    #[serde(default)]
    pub strategy: StrategyConfig,
    #[serde(default)]
    pub risk: RiskConfig,
    #[serde(default)]
    pub polling: PollingConfig,
    #[serde(default)]
    pub price_source: PriceSourceConfig,
    #[serde(default)]
    pub execution: ExecutionConfig,
    #[serde(default)]
    pub timeouts: TimeoutConfig,
    #[serde(default)]
    pub redeem: RedeemConfig,
    #[serde(default)]
    pub misc: MiscConfig,
    /// Two time windows for trade log monitoring (UTC hours, 0-24).
    /// Each window is a half-open interval [start_hour, end_hour).
    /// Supports wrap-around (e.g. start=22, end=6 means 22:00-06:00 UTC).
    #[serde(default)]
    pub time_windows: TimeWindowsConfig,
}

// ─── Trading ───

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TradingMode {
    #[default]
    Paper,
    Live,
}

impl TradingMode {
    pub fn is_paper(self) -> bool {
        matches!(self, Self::Paper)
    }

    pub fn is_live(self) -> bool {
        matches!(self, Self::Live)
    }
}

impl std::fmt::Display for TradingMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Paper => write!(f, "paper"),
            Self::Live => write!(f, "live"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingConfig {
    #[serde(default)]
    pub mode: TradingMode,
    /// Starting balance for paper mode.
    #[serde(
        default = "defaults::paper_starting_balance",
        with = "rust_decimal::serde::float"
    )]
    pub paper_starting_balance: Decimal,
    /// Loaded from PRIVATE_KEY env var (not stored in config)
    #[serde(skip, default = "defaults::private_key")]
    pub private_key: SecretString,
}

// ─── Market ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketConfig {
    #[serde(default = "defaults::stale_threshold_ms")]
    pub stale_threshold_ms: i64,
    #[serde(default = "defaults::min_ttl_ms")]
    pub min_ttl_ms: i64,
}

// ─── Polymarket CLOB ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolymarketConfig {
    #[serde(default = "defaults::gamma_api_url")]
    pub gamma_api_url: String,
}

// ─── Strategy ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    #[serde(
        default = "defaults::extreme_threshold",
        with = "rust_decimal::serde::float"
    )]
    pub extreme_threshold: Decimal,
    #[serde(default = "defaults::fair_value", with = "rust_decimal::serde::float")]
    pub fair_value: Decimal,
    #[serde(
        default = "defaults::position_size_usdc",
        with = "rust_decimal::serde::float"
    )]
    pub position_size_usdc: Decimal,
    /// Minimum entry price to trade (avoid illiquid extreme prices)
    #[serde(
        default = "defaults::min_entry_price",
        with = "rust_decimal::serde::float"
    )]
    pub min_entry_price: Decimal,
    /// Maximum entry price to trade (avoid illiquid extreme prices)
    #[serde(
        default = "defaults::max_entry_price",
        with = "rust_decimal::serde::float"
    )]
    pub max_entry_price: Decimal,
    /// Minimum time-to-live for market to enter a trade (ms)
    #[serde(default = "defaults::min_ttl_for_entry_ms")]
    pub min_ttl_for_entry_ms: u64,
    /// BTC trend lookback window in seconds for momentum confirmation.
    /// 0 = disabled.
    #[serde(default = "defaults::btc_trend_window_s")]
    pub btc_trend_window_s: u64,
    /// Minimum BTC price change (%, as decimal e.g. 0.05 = 0.05%) to consider
    /// a meaningful trend. Trades against the trend are skipped.
    #[serde(
        default = "defaults::btc_trend_min_pct",
        with = "rust_decimal::serde::float"
    )]
    pub btc_trend_min_pct: Decimal,
    /// Sliding-window circuit breaker: number of recent trades to evaluate.
    /// 0 = disabled.
    #[serde(default = "defaults::circuit_breaker_window")]
    pub circuit_breaker_window: u32,
    /// Sliding-window circuit breaker: minimum win rate to keep trading.
    #[serde(
        default = "defaults::circuit_breaker_min_win_rate",
        with = "rust_decimal::serde::float"
    )]
    pub circuit_breaker_min_win_rate: Decimal,
}

// ─── Risk ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    #[serde(default = "defaults::max_fak_retries")]
    pub max_fak_retries: u32,
    #[serde(default = "defaults::fak_backoff_ms")]
    pub fak_backoff_ms: u64,
    /// Daily loss limit in USDC (0 = disabled)
    #[serde(
        default = "defaults::daily_loss_limit",
        with = "rust_decimal::serde::float"
    )]
    pub daily_loss_limit_usdc: Decimal,
}

// ─── Polling ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollingConfig {
    #[serde(default = "defaults::signal_interval_ms")]
    pub signal_interval_ms: u64,
    #[serde(default = "defaults::status_interval_ms")]
    pub status_interval_ms: u64,
    /// Market discovery refresh interval in seconds.
    #[serde(default = "defaults::market_refresh_secs")]
    pub market_refresh_secs: u64,
    /// Settlement check interval in seconds.
    #[serde(default = "defaults::settlement_check_secs")]
    pub settlement_check_secs: u64,
}

// ─── Execution ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    /// Price slippage tolerance (e.g., 0.01 = 1%)
    #[serde(
        default = "defaults::slippage_tolerance",
        with = "rust_decimal::serde::float"
    )]
    pub slippage_tolerance: Decimal,
}

// ─── Price Source ───

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceSourceType {
    #[default]
    Binance,
    BinanceWs,
}

impl std::fmt::Display for PriceSourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Binance => write!(f, "binance"),
            Self::BinanceWs => write!(f, "binance_ws"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceSourceConfig {
    #[serde(default)]
    pub source: PriceSourceType,
    #[serde(default = "defaults::symbol")]
    pub symbol: String,
    /// Maximum number of price ticks retained in the buffer.
    #[serde(default = "defaults::price_buffer_max")]
    pub buffer_max: usize,
    /// Minimum buffer ticks required before the bot starts trading.
    #[serde(default = "defaults::price_buffer_min_ticks")]
    pub buffer_min_ticks: usize,
}

// ─── Timeouts ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutConfig {
    /// Gamma API HTTP request timeout (seconds).
    #[serde(default = "defaults::gamma_http_timeout_secs")]
    pub gamma_http_secs: u64,
    /// WebSocket connect timeout (seconds).
    #[serde(default = "defaults::ws_connect_timeout_secs")]
    pub ws_connect_secs: u64,
    /// WebSocket reconnect max backoff (seconds).
    #[serde(default = "defaults::ws_max_backoff_secs")]
    pub ws_max_backoff_secs: u64,
    /// CLOB price query timeout (seconds).
    #[serde(default = "defaults::clob_price_timeout_secs")]
    pub clob_price_secs: u64,
    /// CLOB authentication timeout (seconds).
    #[serde(default = "defaults::clob_auth_timeout_secs")]
    pub clob_auth_secs: u64,
    /// CLOB order post timeout (seconds).
    #[serde(default = "defaults::clob_order_timeout_secs")]
    pub clob_order_secs: u64,
    /// RPC connect timeout for balance/redeem queries (seconds).
    #[serde(default = "defaults::rpc_connect_timeout_secs")]
    pub rpc_connect_secs: u64,
    /// RPC redeem transaction timeout (seconds).
    #[serde(default = "defaults::rpc_redeem_timeout_secs")]
    pub rpc_redeem_secs: u64,
    /// USDC balanceOf query timeout (seconds).
    #[serde(default = "defaults::balance_query_timeout_secs")]
    pub balance_query_secs: u64,
}

// ─── Redeem ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedeemConfig {
    /// Maximum retry attempts for on-chain redemption.
    #[serde(default = "defaults::redeem_max_retries")]
    pub max_retries: u32,
    /// Delay between successive redemption transactions (seconds).
    #[serde(default = "defaults::redeem_delay_secs")]
    pub delay_secs: u64,
    /// Concurrency limit for redeemable position scanning.
    #[serde(default = "defaults::redeem_concurrency")]
    pub concurrency: usize,
}

// ─── Misc ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiscConfig {
    /// Trade log flush interval (seconds).
    #[serde(default = "defaults::trade_log_flush_secs")]
    pub trade_log_flush_secs: u64,
    /// Graceful shutdown timeout (seconds).
    #[serde(default = "defaults::shutdown_timeout_secs")]
    pub shutdown_timeout_secs: u64,
    /// Number of future 5-minute windows to search during market discovery.
    #[serde(default = "defaults::market_search_windows")]
    pub market_search_windows: u32,
    /// Outcome price threshold to determine a winning resolution (0.0-1.0).
    #[serde(default = "defaults::resolution_price_threshold")]
    pub resolution_price_threshold: f64,
}

// ─── Time Windows ───

/// Configuration for two monitoring time windows.
/// Each window is a half-open interval [start_hour, end_hour) in UTC (0-24).
/// Supports wrap-around midnight (e.g. start=22, end=6 means 22:00-06:00 UTC).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeWindowsConfig {
    /// First monitoring window.
    /// Defaults to [0, 12) — first half of the day.
    #[serde(default = "defaults::window1_start")]
    pub window1_start: u32,
    #[serde(default = "defaults::window1_end")]
    pub window1_end: u32,
    /// Second monitoring window.
    /// Defaults to [12, 24) — second half of the day.
    #[serde(default = "defaults::window2_start")]
    pub window2_start: u32,
    #[serde(default = "defaults::window2_end")]
    pub window2_end: u32,
}

fn is_valid_binance_symbol(symbol: &str) -> bool {
    !symbol.is_empty()
        && symbol
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        && !symbol.contains('-')
}

// ─── Defaults ───

impl Default for TradingConfig {
    fn default() -> Self {
        Self {
            mode: TradingMode::default(),
            paper_starting_balance: defaults::paper_starting_balance(),
            private_key: defaults::private_key(),
        }
    }
}

impl Default for MarketConfig {
    fn default() -> Self {
        Self {
            stale_threshold_ms: defaults::stale_threshold_ms(),
            min_ttl_ms: defaults::min_ttl_ms(),
        }
    }
}

impl Default for PolymarketConfig {
    fn default() -> Self {
        Self {
            gamma_api_url: defaults::gamma_api_url(),
        }
    }
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            extreme_threshold: defaults::extreme_threshold(),
            fair_value: defaults::fair_value(),
            position_size_usdc: defaults::position_size_usdc(),
            min_entry_price: defaults::min_entry_price(),
            max_entry_price: defaults::max_entry_price(),
            min_ttl_for_entry_ms: defaults::min_ttl_for_entry_ms(),
            btc_trend_window_s: defaults::btc_trend_window_s(),
            btc_trend_min_pct: defaults::btc_trend_min_pct(),
            circuit_breaker_window: defaults::circuit_breaker_window(),
            circuit_breaker_min_win_rate: defaults::circuit_breaker_min_win_rate(),
        }
    }
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_fak_retries: defaults::max_fak_retries(),
            fak_backoff_ms: defaults::fak_backoff_ms(),
            daily_loss_limit_usdc: defaults::daily_loss_limit(),
        }
    }
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            slippage_tolerance: defaults::slippage_tolerance(),
        }
    }
}

impl Default for PollingConfig {
    fn default() -> Self {
        Self {
            signal_interval_ms: defaults::signal_interval_ms(),
            status_interval_ms: defaults::status_interval_ms(),
            market_refresh_secs: defaults::market_refresh_secs(),
            settlement_check_secs: defaults::settlement_check_secs(),
        }
    }
}

impl Default for PriceSourceConfig {
    fn default() -> Self {
        Self {
            source: PriceSourceType::Binance,
            symbol: defaults::symbol(),
            buffer_max: defaults::price_buffer_max(),
            buffer_min_ticks: defaults::price_buffer_min_ticks(),
        }
    }
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            gamma_http_secs: defaults::gamma_http_timeout_secs(),
            ws_connect_secs: defaults::ws_connect_timeout_secs(),
            ws_max_backoff_secs: defaults::ws_max_backoff_secs(),
            clob_price_secs: defaults::clob_price_timeout_secs(),
            clob_auth_secs: defaults::clob_auth_timeout_secs(),
            clob_order_secs: defaults::clob_order_timeout_secs(),
            rpc_connect_secs: defaults::rpc_connect_timeout_secs(),
            rpc_redeem_secs: defaults::rpc_redeem_timeout_secs(),
            balance_query_secs: defaults::balance_query_timeout_secs(),
        }
    }
}

impl Default for RedeemConfig {
    fn default() -> Self {
        Self {
            max_retries: defaults::redeem_max_retries(),
            delay_secs: defaults::redeem_delay_secs(),
            concurrency: defaults::redeem_concurrency(),
        }
    }
}

impl Default for MiscConfig {
    fn default() -> Self {
        Self {
            trade_log_flush_secs: defaults::trade_log_flush_secs(),
            shutdown_timeout_secs: defaults::shutdown_timeout_secs(),
            market_search_windows: defaults::market_search_windows(),
            resolution_price_threshold: defaults::resolution_price_threshold(),
        }
    }
}

impl Default for TimeWindowsConfig {
    fn default() -> Self {
        Self {
            window1_start: defaults::window1_start(),
            window1_end: defaults::window1_end(),
            window2_start: defaults::window2_start(),
            window2_end: defaults::window2_end(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        let mut config: Config = toml::from_str(&content)?;
        // Load secrets from env (not stored in config)
        if let Ok(pk) = std::env::var("PRIVATE_KEY") {
            config.trading.private_key = SecretString::new(pk.into());
        }
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        let zero = Decimal::ZERO;
        let one = Decimal::ONE;

        if self.polling.signal_interval_ms == 0 {
            anyhow::bail!("polling.signal_interval_ms must be > 0");
        }
        if !(zero < self.strategy.extreme_threshold && self.strategy.extreme_threshold < one) {
            anyhow::bail!("strategy.extreme_threshold must be in (0, 1)");
        }
        if !(zero < self.strategy.fair_value && self.strategy.fair_value < one) {
            anyhow::bail!("strategy.fair_value must be in (0, 1)");
        }
        // Validate threshold > fair_value (otherwise edge can never be positive)
        if self.strategy.extreme_threshold <= self.strategy.fair_value {
            anyhow::bail!(
                "strategy.extreme_threshold ({}) must be > fair_value ({})",
                self.strategy.extreme_threshold,
                self.strategy.fair_value
            );
        }
        if self.strategy.extreme_threshold < dec("0.80") {
            tracing::warn!(
                "extreme_threshold < 0.80 (current: {}) — this bot targets extreme markets, consider >= 0.90",
                self.strategy.extreme_threshold
            );
        }
        if self.strategy.position_size_usdc <= zero {
            anyhow::bail!("strategy.position_size_usdc must be > 0");
        }
        if !(zero < self.strategy.min_entry_price
            && self.strategy.min_entry_price < self.strategy.max_entry_price
            && self.strategy.max_entry_price < one)
        {
            anyhow::bail!(
                "strategy.min_entry_price and max_entry_price must satisfy: 0 < min_entry_price < max_entry_price < 1"
            );
        }
        if self.strategy.min_ttl_for_entry_ms == 0 {
            anyhow::bail!("strategy.min_ttl_for_entry_ms must be > 0");
        }
        if self.strategy.circuit_breaker_window > 0
            && self.strategy.circuit_breaker_min_win_rate > one
        {
            anyhow::bail!("strategy.circuit_breaker_min_win_rate must be in [0, 1]");
        }
        // Ring buffer cap in decider is 200; window must not exceed it.
        const RECENT_RESULTS_CAP: u32 = 200;
        if self.strategy.circuit_breaker_window > RECENT_RESULTS_CAP {
            anyhow::bail!(
                "strategy.circuit_breaker_window ({}) must be <= {} (ring buffer cap)",
                self.strategy.circuit_breaker_window,
                RECENT_RESULTS_CAP
            );
        }
        if !is_valid_binance_symbol(&self.price_source.symbol) {
            anyhow::bail!(
                "price_source.symbol must match Binance format like BTCUSDT (got {})",
                self.price_source.symbol
            );
        }
        if self.price_source.buffer_max == 0 {
            anyhow::bail!("price_source.buffer_max must be > 0");
        }
        if self.price_source.buffer_min_ticks > self.price_source.buffer_max {
            anyhow::bail!(
                "price_source.buffer_min_ticks ({}) must be <= buffer_max ({})",
                self.price_source.buffer_min_ticks,
                self.price_source.buffer_max
            );
        }
        if self.misc.resolution_price_threshold <= 0.0 || self.misc.resolution_price_threshold > 1.0
        {
            anyhow::bail!("misc.resolution_price_threshold must be in (0, 1]");
        }

        // Validate time windows (start == end is valid — means 24h full-day window)
        let tw = &self.time_windows;
        if tw.window1_start > 24 || tw.window1_end > 24 {
            anyhow::bail!(
                "time_windows.window1 hours must be in 0-24 range (got start={}, end={})",
                tw.window1_start,
                tw.window1_end
            );
        }
        if tw.window2_start > 24 || tw.window2_end > 24 {
            anyhow::bail!(
                "time_windows.window2 hours must be in 0-24 range (got start={}, end={})",
                tw.window2_start,
                tw.window2_end
            );
        }
        if tw.window1_start == tw.window1_end && tw.window2_start == tw.window2_end {
            anyhow::bail!("time_windows: at least one window must have non-zero duration");
        }

        Ok(())
    }

    pub fn is_default_non_trading(&self) -> bool {
        self.strategy.extreme_threshold == defaults::extreme_threshold()
            && self.strategy.position_size_usdc == defaults::position_size_usdc()
            && self.risk.daily_loss_limit_usdc == defaults::daily_loss_limit()
    }
}

// ─── Tests ───

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_rejects_zero_interval() {
        let mut cfg = Config::default();
        cfg.polling.signal_interval_ms = 0;

        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_validate_accepts_defaults() {
        let cfg = Config::default();

        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_trading_mode_serde_roundtrip() {
        let toml_str = r#"mode = "live""#;
        let cfg: TradingConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.mode, TradingMode::Live);

        let toml_str = r#"mode = "paper""#;
        let cfg: TradingConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.mode, TradingMode::Paper);
    }

    #[test]
    fn test_trading_mode_default_is_paper() {
        let cfg = TradingConfig::default();
        assert_eq!(cfg.mode, TradingMode::Paper);
    }

    #[test]
    fn test_trading_mode_is_paper_and_live() {
        assert!(TradingMode::Paper.is_paper());
        assert!(!TradingMode::Paper.is_live());
        assert!(TradingMode::Live.is_live());
        assert!(!TradingMode::Live.is_paper());
    }

    #[test]
    fn test_validate_rejects_binance_symbol_with_dash() {
        let mut cfg = Config::default();
        cfg.price_source.symbol = "BTC-USD".to_string();

        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_zero_position_size_usdc() {
        let mut cfg = Config::default();
        cfg.strategy.position_size_usdc = Decimal::ZERO;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_max_entry_price_not_above_min() {
        let mut cfg = Config::default();
        cfg.strategy.min_entry_price = dec("0.08");
        cfg.strategy.max_entry_price = dec("0.08");

        let err = cfg.validate().expect_err("expected validation failure");
        assert!(err.to_string().contains("min_entry_price"));
    }

    #[test]
    fn test_validate_rejects_zero_min_ttl_for_entry_ms() {
        let mut cfg = Config::default();
        cfg.strategy.min_ttl_for_entry_ms = 0;

        let err = cfg.validate().expect_err("expected validation failure");
        assert!(err.to_string().contains("min_ttl_for_entry_ms"));
    }

    #[test]
    fn test_validate_rejects_non_positive_min_entry_price() {
        let mut cfg = Config::default();
        cfg.strategy.min_entry_price = Decimal::ZERO;

        let err = cfg.validate().expect_err("expected validation failure");
        assert!(err.to_string().contains("min_entry_price"));
    }

    #[test]
    fn test_validate_rejects_max_entry_price_at_or_above_one() {
        let mut cfg = Config::default();
        cfg.strategy.max_entry_price = Decimal::ONE;

        let err = cfg.validate().expect_err("expected validation failure");
        assert!(err.to_string().contains("max_entry_price"));
    }

    #[test]
    fn test_validate_rejects_buffer_min_exceeds_max() {
        let mut cfg = Config::default();
        cfg.price_source.buffer_min_ticks = 2000;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_time_windows_accepts_defaults() {
        let cfg = Config::default();
        assert!(cfg.validate().is_ok());
        assert_eq!(cfg.time_windows.window1_start, 0);
        assert_eq!(cfg.time_windows.window1_end, 12);
        assert_eq!(cfg.time_windows.window2_start, 12);
        assert_eq!(cfg.time_windows.window2_end, 24);
    }

    #[test]
    fn test_time_windows_rejects_hour_above_24() {
        let mut cfg = Config::default();
        cfg.time_windows.window1_start = 25;
        let err = cfg.validate().expect_err("expected validation failure");
        assert!(err.to_string().contains("window1"));
    }

    #[test]
    fn test_time_windows_rejects_both_zero_duration() {
        let mut cfg = Config::default();
        cfg.time_windows.window1_start = 8;
        cfg.time_windows.window1_end = 8;
        cfg.time_windows.window2_start = 16;
        cfg.time_windows.window2_end = 16;
        let err = cfg.validate().expect_err("expected validation failure");
        assert!(err.to_string().contains("non-zero duration"));
    }

    #[test]
    fn test_time_windows_allows_wrap_around() {
        let mut cfg = Config::default();
        cfg.time_windows.window1_start = 22;
        cfg.time_windows.window1_end = 6;
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_time_windows_allows_one_zero_duration_window() {
        let mut cfg = Config::default();
        cfg.time_windows.window1_start = 10;
        cfg.time_windows.window1_end = 10;
        // window2 still has non-zero duration
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_time_windows_toml_roundtrip() {
        let toml_str = r#"
window1_start = 6
window1_end = 18
window2_start = 18
window2_end = 6
"#;
        let tw: TimeWindowsConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(tw.window1_start, 6);
        assert_eq!(tw.window1_end, 18);
        assert_eq!(tw.window2_start, 18);
        assert_eq!(tw.window2_end, 6);
    }

    #[test]
    fn test_full_config_toml_roundtrip() {
        let cfg = Config::default();
        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert!(parsed.validate().is_ok());
    }
}
