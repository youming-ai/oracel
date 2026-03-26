//! Bot configuration

use rust_decimal::Decimal;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

fn dec(s: &str) -> Decimal {
    Decimal::from_str_exact(s).expect("valid decimal literal")
}

mod defaults {
    use super::*;

    pub fn stale_threshold_ms() -> i64 {
        30_000
    }
    pub fn min_ttl_ms() -> i64 {
        30_000
    }
    pub fn extreme_threshold() -> Decimal {
        dec("0.80")
    }
    pub fn fair_value() -> Decimal {
        dec("0.50")
    }
    pub fn position_size_usdc() -> Decimal {
        dec("1.0")
    }
    pub fn min_edge() -> Decimal {
        dec("0.05")
    }
    pub fn min_entry_price() -> Decimal {
        dec("0.08")
    }
    pub fn max_entry_price() -> Decimal {
        dec("0.12")
    }
    pub fn min_ttl_for_entry_ms() -> u64 {
        120_000
    }
    pub fn spot_momentum_30s_threshold() -> Decimal {
        dec("40")
    }
    pub fn spot_momentum_60s_threshold() -> Decimal {
        dec("70")
    }
    pub fn daily_loss_limit() -> Decimal {
        dec("0")
    }
    pub fn max_fak_retries() -> u32 {
        3
    }
    pub fn fak_backoff_ms() -> u64 {
        3_000
    }
    pub fn status_interval_ms() -> u64 {
        10_000
    }
    pub fn slippage_tolerance() -> Decimal {
        dec("0.01")
    }
    pub fn symbol() -> String {
        "BTCUSDT".to_string()
    }
    pub fn private_key() -> SecretString {
        SecretString::new(String::new().into())
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Config {
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
    /// Loaded from PRIVATE_KEY env var (not stored in config.json)
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
    /// Minimum edge required to trade (default 0.05 = 5%)
    #[serde(default = "defaults::min_edge", with = "rust_decimal::serde::float")]
    pub min_edge: Decimal,
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
    /// Spot price momentum threshold over 30s (in USD)
    #[serde(
        default = "defaults::spot_momentum_30s_threshold",
        with = "rust_decimal::serde::float"
    )]
    pub spot_momentum_30s_threshold: Decimal,
    /// Spot price momentum threshold over 60s (in USD)
    #[serde(
        default = "defaults::spot_momentum_60s_threshold",
        with = "rust_decimal::serde::float"
    )]
    pub spot_momentum_60s_threshold: Decimal,
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
    pub signal_interval_ms: u64,
    #[serde(default = "defaults::status_interval_ms")]
    pub status_interval_ms: u64,
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
    Coinbase,
    CoinbaseWs,
}

impl PriceSourceType {
    pub fn expects_dash_symbol(self) -> bool {
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
pub struct PriceSourceConfig {
    #[serde(default)]
    pub source: PriceSourceType,
    #[serde(default = "defaults::symbol")]
    pub symbol: String,
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
            gamma_api_url: "https://gamma-api.polymarket.com".to_string(),
        }
    }
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            extreme_threshold: defaults::extreme_threshold(),
            fair_value: defaults::fair_value(),
            position_size_usdc: defaults::position_size_usdc(),
            min_edge: defaults::min_edge(),
            min_entry_price: defaults::min_entry_price(),
            max_entry_price: defaults::max_entry_price(),
            min_ttl_for_entry_ms: defaults::min_ttl_for_entry_ms(),
            spot_momentum_30s_threshold: defaults::spot_momentum_30s_threshold(),
            spot_momentum_60s_threshold: defaults::spot_momentum_60s_threshold(),
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
            signal_interval_ms: 1000,
            status_interval_ms: defaults::status_interval_ms(),
        }
    }
}

impl Default for PriceSourceConfig {
    fn default() -> Self {
        Self {
            source: PriceSourceType::Binance,
            symbol: defaults::symbol(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        let mut config: Config = serde_json::from_str(&content)?;
        // Load secrets from env (not stored in config.json)
        if let Ok(pk) = std::env::var("PRIVATE_KEY") {
            config.trading.private_key = SecretString::new(pk.into());
        }
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let content = serde_json::to_string_pretty(self)?;
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
        if self.strategy.min_edge < zero || self.strategy.min_edge >= one {
            anyhow::bail!("strategy.min_edge must be in [0, 1)");
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
        if self.strategy.spot_momentum_30s_threshold <= zero {
            anyhow::bail!("strategy.spot_momentum_30s_threshold must be > 0");
        }
        if self.strategy.spot_momentum_60s_threshold <= zero {
            anyhow::bail!("strategy.spot_momentum_60s_threshold must be > 0");
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
    fn test_validate_rejects_non_positive_spot_momentum_30s_threshold() {
        let mut cfg = Config::default();
        cfg.strategy.spot_momentum_30s_threshold = Decimal::ZERO;

        let err = cfg.validate().expect_err("expected validation failure");
        assert!(err.to_string().contains("spot_momentum_30s_threshold"));
    }

    #[test]
    fn test_validate_rejects_non_positive_spot_momentum_60s_threshold() {
        let mut cfg = Config::default();
        cfg.strategy.spot_momentum_60s_threshold = Decimal::ZERO;

        let err = cfg.validate().expect_err("expected validation failure");
        assert!(err.to_string().contains("spot_momentum_60s_threshold"));
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
}
