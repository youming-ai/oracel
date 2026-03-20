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
    pub edge: EdgeConfigFile,
    pub risk: RiskConfig,
    pub polling: PollingConfig,
    #[serde(default)]
    pub price_source: PriceSourceConfig,
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
    pub window_minutes: f64,
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
    #[serde(default = "default_btc_tiebreaker_usd")]
    pub btc_tiebreaker_usd: f64,
    #[serde(
        default = "default_momentum_threshold",
        with = "rust_decimal::serde::float"
    )]
    pub momentum_threshold: Decimal,
    #[serde(default = "default_momentum_lookback_ms")]
    pub momentum_lookback_ms: i64,
    #[serde(default = "default_max_position", with = "rust_decimal::serde::float")]
    pub max_position: Decimal,
    #[serde(default = "default_min_position", with = "rust_decimal::serde::float")]
    pub min_position: Decimal,
    #[serde(
        default = "default_max_risk_fraction",
        with = "rust_decimal::serde::float"
    )]
    pub max_risk_fraction: Decimal,
}

fn default_extreme_threshold() -> Decimal {
    dec("0.80")
}
fn default_fair_value() -> Decimal {
    dec("0.50")
}
fn default_btc_tiebreaker_usd() -> f64 {
    5.0
}
fn default_momentum_threshold() -> Decimal {
    dec("0.001")
}
fn default_momentum_lookback_ms() -> i64 {
    120_000
}
fn default_max_position() -> Decimal {
    dec("10.0")
}
fn default_min_position() -> Decimal {
    dec("1.0")
}
fn default_max_risk_fraction() -> Decimal {
    dec("0.10")
}

// ─── Edge Thresholds ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EdgeConfigFile {
    #[serde(with = "rust_decimal::serde::float")]
    pub edge_threshold_early: Decimal,
}

// ─── Risk ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RiskConfig {
    pub max_consecutive_losses: u32,
    #[serde(
        default = "default_max_daily_loss_pct",
        with = "rust_decimal::serde::float"
    )]
    pub max_daily_loss_pct: Decimal,
    #[serde(default = "default_cooldown_ms")]
    pub cooldown_ms: i64,
}

fn default_max_daily_loss_pct() -> Decimal {
    dec("0.25")
}
fn default_cooldown_ms() -> i64 {
    5_000
}

// ─── Polling ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PollingConfig {
    pub signal_interval_ms: u64,
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
            window_minutes: 5.0,
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
            btc_tiebreaker_usd: 5.0,
            momentum_threshold: dec("0.001"),
            momentum_lookback_ms: 120_000,
            max_position: dec("10.0"),
            min_position: dec("1.0"),
            max_risk_fraction: dec("0.10"),
        }
    }
}

impl Default for EdgeConfigFile {
    fn default() -> Self {
        Self {
            edge_threshold_early: dec("0.15"),
        }
    }
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_consecutive_losses: 8,
            max_daily_loss_pct: dec("0.25"),
            cooldown_ms: 5_000,
        }
    }
}

impl Default for PollingConfig {
    fn default() -> Self {
        Self {
            signal_interval_ms: 1000,
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
        if !(zero < self.risk.max_daily_loss_pct && self.risk.max_daily_loss_pct <= one) {
            anyhow::bail!("risk.max_daily_loss_pct must be in (0, 1]");
        }
        if self.market.window_minutes <= 0.0 {
            anyhow::bail!("market.window_minutes must be > 0");
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

        self.market.window_minutes == defaults.market.window_minutes
            && self.polyclob.gamma_api_url == defaults.polyclob.gamma_api_url
            && self.strategy.extreme_threshold == defaults.strategy.extreme_threshold
            && self.strategy.fair_value == defaults.strategy.fair_value
            && self.strategy.btc_tiebreaker_usd == defaults.strategy.btc_tiebreaker_usd
            && self.strategy.momentum_threshold == defaults.strategy.momentum_threshold
            && self.strategy.momentum_lookback_ms == defaults.strategy.momentum_lookback_ms
            && self.strategy.max_position == defaults.strategy.max_position
            && self.strategy.min_position == defaults.strategy.min_position
            && self.strategy.max_risk_fraction == defaults.strategy.max_risk_fraction
            && self.edge.edge_threshold_early == defaults.edge.edge_threshold_early
            && self.risk.max_consecutive_losses == defaults.risk.max_consecutive_losses
            && self.risk.max_daily_loss_pct == defaults.risk.max_daily_loss_pct
            && self.risk.cooldown_ms == defaults.risk.cooldown_ms
            && self.polling.signal_interval_ms == defaults.polling.signal_interval_ms
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
}
