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

    // ─── Polymarket CLOB ───
    pub fn gamma_api_url() -> String {
        "https://gamma-api.polymarket.com".to_string()
    }
    pub fn gamma_http_secs() -> u64 {
        10
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

    // ─── Redeem ───
    pub fn redeem_max_retries() -> u32 {
        10
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

    pub fn private_key() -> SecretString {
        SecretString::new(String::new().into())
    }
}

// ─── Top-level Config ───

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub trading: TradingConfig,
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
    pub redeem: RedeemConfig,
    #[serde(default)]
    pub misc: MiscConfig,
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
#[serde(deny_unknown_fields)]
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

// ─── Polymarket CLOB ───

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolymarketConfig {
    #[serde(default = "defaults::gamma_api_url")]
    pub gamma_api_url: String,
    #[serde(default = "defaults::gamma_http_secs")]
    pub gamma_http_secs: u64,
}

// ─── Strategy ───

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// Maximum premium over the current CLOB buy quote.
    #[serde(
        default = "defaults::slippage_tolerance",
        with = "rust_decimal::serde::float"
    )]
    pub slippage_tolerance: Decimal,
}

// ─── Risk ───

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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

// ─── Price Source ───

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PriceSourceConfig {
    #[serde(default = "defaults::symbol")]
    pub symbol: String,
    /// Maximum age of the latest Binance tick before trading stops.
    #[serde(default = "defaults::stale_threshold_ms")]
    pub stale_threshold_ms: i64,
    /// Maximum number of price ticks retained in the buffer.
    #[serde(default = "defaults::price_buffer_max")]
    pub buffer_max: usize,
    /// Minimum buffer ticks required before the bot starts trading.
    #[serde(default = "defaults::price_buffer_min_ticks")]
    pub buffer_min_ticks: usize,
}

// ─── Redeem ───

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedeemConfig {
    /// Maximum retry attempts for on-chain redemption.
    #[serde(default = "defaults::redeem_max_retries")]
    pub max_retries: u32,
}

// ─── Misc ───

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

impl Default for PolymarketConfig {
    fn default() -> Self {
        Self {
            gamma_api_url: defaults::gamma_api_url(),
            gamma_http_secs: defaults::gamma_http_secs(),
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
            slippage_tolerance: defaults::slippage_tolerance(),
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
            symbol: defaults::symbol(),
            stale_threshold_ms: defaults::stale_threshold_ms(),
            buffer_max: defaults::price_buffer_max(),
            buffer_min_ticks: defaults::price_buffer_min_ticks(),
        }
    }
}

impl Default for RedeemConfig {
    fn default() -> Self {
        Self {
            max_retries: defaults::redeem_max_retries(),
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

        if self.trading.paper_starting_balance <= zero {
            anyhow::bail!("trading.paper_starting_balance must be > 0");
        }
        if self.price_source.stale_threshold_ms <= 0 {
            anyhow::bail!("price_source.stale_threshold_ms must be > 0");
        }
        if self.polyclob.gamma_api_url.trim().is_empty() || self.polyclob.gamma_http_secs == 0 {
            anyhow::bail!("polyclob URL must not be empty and gamma_http_secs must be > 0");
        }
        if self.polling.signal_interval_ms == 0
            || self.polling.status_interval_ms == 0
            || self.polling.market_refresh_secs == 0
            || self.polling.settlement_check_secs == 0
        {
            anyhow::bail!("all polling intervals must be > 0");
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
        if self.strategy.position_size_usdc < one {
            anyhow::bail!("strategy.position_size_usdc must be >= 1 USDC");
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
        if self.strategy.btc_trend_min_pct < zero {
            anyhow::bail!("strategy.btc_trend_min_pct must be >= 0");
        }
        if self.strategy.circuit_breaker_min_win_rate < zero
            || self.strategy.circuit_breaker_min_win_rate > one
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
        if self.price_source.buffer_max == 0 || self.price_source.buffer_min_ticks == 0 {
            anyhow::bail!("price source buffer sizes must be > 0");
        }
        if self.price_source.buffer_min_ticks > self.price_source.buffer_max {
            anyhow::bail!(
                "price_source.buffer_min_ticks ({}) must be <= buffer_max ({})",
                self.price_source.buffer_min_ticks,
                self.price_source.buffer_max
            );
        }
        if self.strategy.slippage_tolerance < zero || self.strategy.slippage_tolerance >= one {
            anyhow::bail!("strategy.slippage_tolerance must be in [0, 1)");
        }
        if self.risk.daily_loss_limit_usdc < zero || self.risk.max_fak_retries == 0 {
            anyhow::bail!(
                "risk.daily_loss_limit_usdc must be >= 0 and max_fak_retries must be > 0"
            );
        }
        if self.redeem.max_retries == 0
            || self.misc.trade_log_flush_secs == 0
            || self.misc.shutdown_timeout_secs == 0
            || self.misc.market_search_windows == 0
        {
            anyhow::bail!("redeem retries and misc intervals/counts must be > 0");
        }
        if self.misc.resolution_price_threshold <= 0.0 || self.misc.resolution_price_threshold > 1.0
        {
            anyhow::bail!("misc.resolution_price_threshold must be in (0, 1]");
        }

        Ok(())
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
    fn test_rejects_removed_or_unknown_fields() {
        let result = toml::from_str::<Config>(
            r#"
[market]
stale_threshold_ms = 30000
min_ttl_ms = 30000
"#,
        );

        assert!(result.is_err());
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
    fn test_full_config_toml_roundtrip() {
        let cfg = Config::default();
        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert!(parsed.validate().is_ok());
    }
}
