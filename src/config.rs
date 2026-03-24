//! Bot configuration

use rust_decimal::Decimal;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

fn dec(s: &str) -> Decimal {
    Decimal::from_str_exact(s).expect("valid decimal literal")
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Config {
    #[serde(default)]
    pub trading: TradingConfig,
    pub market: MarketConfig,
    pub polyclob: PolymarketConfig,
    pub strategy: StrategyConfig,
    pub risk: RiskConfig,
    pub polling: PollingConfig,
    #[serde(default)]
    pub price_source: PriceSourceConfig,
    #[serde(default)]
    pub execution: ExecutionConfig,
}

// ─── Trading ───

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum TradingMode {
    #[default]
    Paper,
    Live,
}

impl TradingMode {
    pub(crate) fn is_paper(self) -> bool {
        matches!(self, Self::Paper)
    }

    pub(crate) fn is_live(self) -> bool {
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
pub(crate) struct TradingConfig {
    #[serde(default)]
    pub mode: TradingMode,
    /// Loaded from PRIVATE_KEY env var (not stored in config.json)
    #[serde(skip, default = "default_private_key")]
    pub private_key: SecretString,
}

fn default_private_key() -> SecretString {
    SecretString::new(String::new().into())
}

// ─── Market ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MarketConfig {
    #[serde(default = "default_stale_threshold_ms")]
    pub stale_threshold_ms: i64,
    #[serde(default = "default_min_ttl_ms")]
    pub min_ttl_ms: i64,
}

fn default_stale_threshold_ms() -> i64 {
    30_000
}
fn default_min_ttl_ms() -> i64 {
    30_000
}

// ─── Polymarket CLOB ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PolymarketConfig {
    pub gamma_api_url: String,
}

// ─── Strategy ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StrategyConfig {
    #[serde(
        default = "default_extreme_threshold",
        with = "rust_decimal::serde::float"
    )]
    pub extreme_threshold: Decimal,
    #[serde(default = "default_fair_value", with = "rust_decimal::serde::float")]
    pub fair_value: Decimal,
    #[serde(
        default = "default_position_size_usdc",
        with = "rust_decimal::serde::float"
    )]
    pub position_size_usdc: Decimal,
    /// Minimum edge required to trade (default 0.05 = 5%)
    #[serde(default = "default_min_edge", with = "rust_decimal::serde::float")]
    pub min_edge: Decimal,
}

fn default_extreme_threshold() -> Decimal {
    dec("0.80")
}
fn default_fair_value() -> Decimal {
    dec("0.50")
}
fn default_position_size_usdc() -> Decimal {
    dec("1.0")
}
fn default_min_edge() -> Decimal {
    dec("0.05")
}
// ─── Risk ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RiskConfig {
    #[serde(default = "default_max_fok_retries")]
    pub max_fok_retries: u32,
    #[serde(default = "default_fok_backoff_ms")]
    pub fok_backoff_ms: u64,
    /// Daily loss limit in USDC (0 = disabled)
    #[serde(
        default = "default_daily_loss_limit",
        with = "rust_decimal::serde::float"
    )]
    pub daily_loss_limit_usdc: Decimal,
}

fn default_daily_loss_limit() -> Decimal {
    dec("0") // disabled by default
}

fn default_max_fok_retries() -> u32 {
    3
}

fn default_fok_backoff_ms() -> u64 {
    3_000
}

// ─── Polling ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PollingConfig {
    pub signal_interval_ms: u64,
    #[serde(default = "default_status_interval_ms")]
    pub status_interval_ms: u64,
}

fn default_status_interval_ms() -> u64 {
    10_000
}

// ─── Execution ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ExecutionConfig {
    /// Price slippage tolerance (e.g., 0.01 = 1%)
    #[serde(
        default = "default_slippage_tolerance",
        with = "rust_decimal::serde::float"
    )]
    pub slippage_tolerance: Decimal,
}

fn default_slippage_tolerance() -> Decimal {
    dec("0.01") // 1%
}

// ─── Price Source ───

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PriceSourceType {
    #[default]
    Binance,
    BinanceWs,
    Coinbase,
    CoinbaseWs,
}

impl PriceSourceType {
    pub(crate) fn expects_dash_symbol(self) -> bool {
        matches!(self, Self::Coinbase | Self::CoinbaseWs)
    }
}

impl std::fmt::Display for PriceSourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Binance => write!(f, "binance"),
            Self::BinanceWs => write!(f, "binance_ws"),
            Self::Coinbase => write!(f, "coinbase"),
            Self::CoinbaseWs => write!(f, "coinbase_ws"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PriceSourceConfig {
    #[serde(default)]
    pub source: PriceSourceType,
    #[serde(default = "default_symbol")]
    pub symbol: String,
}

fn default_symbol() -> String {
    "BTCUSDT".to_string()
}

fn is_valid_binance_symbol(symbol: &str) -> bool {
    !symbol.is_empty()
        && symbol
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        && !symbol.contains('-')
}

fn is_valid_coinbase_symbol(symbol: &str) -> bool {
    let mut parts = symbol.split('-');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(base), Some(quote), None) => {
            !base.is_empty()
                && !quote.is_empty()
                && base
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
                && quote
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        }
        _ => false,
    }
}

// ─── Defaults ───

impl Default for TradingConfig {
    fn default() -> Self {
        Self {
            mode: TradingMode::default(),
            private_key: default_private_key(),
        }
    }
}

impl Default for MarketConfig {
    fn default() -> Self {
        Self {
            stale_threshold_ms: 30_000,
            min_ttl_ms: 30_000,
        }
    }
}

impl Default for PolymarketConfig {
    fn default() -> Self {
        Self {
            gamma_api_url: "https://gamma-api.polymarket.com".to_string(),
        }
    }
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            extreme_threshold: dec("0.80"),
            fair_value: dec("0.50"),
            position_size_usdc: dec("1.0"),
            min_edge: dec("0.05"),
        }
    }
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_fok_retries: 3,
            fok_backoff_ms: 3_000,
            daily_loss_limit_usdc: dec("0"),
        }
    }
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            slippage_tolerance: dec("0.01"),
        }
    }
}

impl Default for PollingConfig {
    fn default() -> Self {
        Self {
            signal_interval_ms: 1000,
            status_interval_ms: 10_000,
        }
    }
}

impl Default for PriceSourceConfig {
    fn default() -> Self {
        Self {
            source: PriceSourceType::Binance,
            symbol: default_symbol(),
        }
    }
}

impl Config {
    pub(crate) fn load(path: &Path) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        let mut config: Config = serde_json::from_str(&content)?;
        // Load secrets from env (not stored in config.json)
        if let Ok(pk) = std::env::var("PRIVATE_KEY") {
            config.trading.private_key = SecretString::new(pk.into());
        }
        Ok(config)
    }

    pub(crate) fn save(&self, path: &Path) -> anyhow::Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    pub(crate) fn validate(&self) -> anyhow::Result<()> {
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
        // Warn if threshold is very high (may result in few trades)
        if self.strategy.extreme_threshold > dec("0.95") {
            tracing::warn!(
                "extreme_threshold > 0.95 (current: {}) may result in very few trades",
                self.strategy.extreme_threshold
            );
        }
        if self.strategy.position_size_usdc <= zero {
            anyhow::bail!("strategy.position_size_usdc must be > 0");
        }
        if self.price_source.source.expects_dash_symbol() {
            if !is_valid_coinbase_symbol(&self.price_source.symbol) {
                anyhow::bail!(
                    "price_source.symbol must match Coinbase format like BTC-USD when source={} (got {})",
                    self.price_source.source,
                    self.price_source.symbol
                );
            }
        } else if !is_valid_binance_symbol(&self.price_source.symbol) {
            anyhow::bail!(
                "price_source.symbol must match Binance format like BTCUSDT when source={} (got {})",
                self.price_source.source,
                self.price_source.symbol
            );
        }

        Ok(())
    }

    pub(crate) fn is_default_non_trading(&self) -> bool {
        let defaults = Config::default();

        self.market.stale_threshold_ms == defaults.market.stale_threshold_ms
            && self.market.min_ttl_ms == defaults.market.min_ttl_ms
            && self.polyclob.gamma_api_url == defaults.polyclob.gamma_api_url
            && self.strategy.extreme_threshold == defaults.strategy.extreme_threshold
            && self.strategy.fair_value == defaults.strategy.fair_value
            && self.strategy.position_size_usdc == defaults.strategy.position_size_usdc
            && self.risk.max_fok_retries == defaults.risk.max_fok_retries
            && self.polling.signal_interval_ms == defaults.polling.signal_interval_ms
            && self.polling.status_interval_ms == defaults.polling.status_interval_ms
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
        let json = r#"{"mode":"live"}"#;
        let cfg: TradingConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.mode, TradingMode::Live);

        let json = r#"{"mode":"paper"}"#;
        let cfg: TradingConfig = serde_json::from_str(json).unwrap();
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
    fn test_validate_rejects_coinbase_symbol_in_binance_format() {
        let mut cfg = Config::default();
        cfg.price_source.source = PriceSourceType::Coinbase;
        cfg.price_source.symbol = "BTCUSDT".to_string();

        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_binance_symbol_in_coinbase_format() {
        let mut cfg = Config::default();
        cfg.price_source.source = PriceSourceType::Binance;
        cfg.price_source.symbol = "BTC-USD".to_string();

        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_validate_accepts_coinbase_symbol_format() {
        let mut cfg = Config::default();
        cfg.price_source.source = PriceSourceType::Coinbase;
        cfg.price_source.symbol = "BTC-USD".to_string();

        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_validate_rejects_zero_position_size_usdc() {
        let mut cfg = Config::default();
        cfg.strategy.position_size_usdc = Decimal::ZERO;
        assert!(cfg.validate().is_err());
    }
}
