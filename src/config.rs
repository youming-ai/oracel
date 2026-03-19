//! Bot configuration

use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub trading: TradingConfig,
    pub market: MarketConfig,
    pub polyclob: PolymarketConfig,
    pub strategy: StrategyConfig,
    pub edge: EdgeConfigFile,
    pub risk: RiskConfig,
    pub polling: PollingConfig,
}

// ─── Trading ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingConfig {
    pub mode: String,
    /// Loaded from PRIVATE_KEY env var (not stored in config.json)
    #[serde(skip, default = "default_private_key")]
    pub private_key: SecretString,
}

fn default_private_key() -> SecretString {
    SecretString::new(String::new().into())
}

// ─── Market ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketConfig {
    #[serde(default)]
    pub event_url: String,
    pub series_id: String,
    pub window_minutes: f64,
}

// ─── Polymarket CLOB ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolymarketConfig {
    pub gamma_api_url: String,
}

// ─── Strategy ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    pub max_position_size: f64,
    pub min_order_size: f64,
    #[serde(default = "default_extreme_threshold")]
    pub extreme_threshold: f64,
    #[serde(default = "default_fair_value")]
    pub fair_value: f64,
    #[serde(default = "default_btc_tiebreaker_usd")]
    pub btc_tiebreaker_usd: f64,
    #[serde(default = "default_momentum_threshold")]
    pub momentum_threshold: f64,
    #[serde(default = "default_momentum_lookback_ms")]
    pub momentum_lookback_ms: i64,
}

fn default_extreme_threshold() -> f64 {
    0.80
}
fn default_fair_value() -> f64 {
    0.50
}
fn default_btc_tiebreaker_usd() -> f64 {
    5.0
}
fn default_momentum_threshold() -> f64 {
    0.001
}
fn default_momentum_lookback_ms() -> i64 {
    120_000
}

// ─── Edge Thresholds ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeConfigFile {
    pub edge_threshold_early: f64,
    #[serde(default = "default_edge_threshold_mid")]
    pub edge_threshold_mid: f64,
    #[serde(default = "default_edge_threshold_late")]
    pub edge_threshold_late: f64,
    #[serde(default = "default_min_prob")]
    pub min_prob_early: f64,
    #[serde(default = "default_min_prob")]
    pub min_prob_mid: f64,
    #[serde(default = "default_min_prob")]
    pub min_prob_late: f64,
}

fn default_edge_threshold_mid() -> f64 {
    0.15
}
fn default_edge_threshold_late() -> f64 {
    0.20
}
fn default_min_prob() -> f64 {
    0.50
}

// ─── Risk ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    pub max_daily_loss_usdc: f64,
    pub max_consecutive_losses: u32,
    #[serde(default = "default_max_daily_loss_pct")]
    pub max_daily_loss_pct: f64,
    #[serde(default = "default_cooldown_ms")]
    pub cooldown_ms: i64,
    #[serde(default = "default_max_risk_fraction")]
    pub max_risk_fraction: f64,
}

fn default_max_daily_loss_pct() -> f64 {
    0.10
}
fn default_cooldown_ms() -> i64 {
    5_000
}
fn default_max_risk_fraction() -> f64 {
    0.10
}

// ─── Polling ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollingConfig {
    pub signal_interval_ms: u64,
}

// ─── Defaults ───

impl Default for Config {
    fn default() -> Self {
        Self {
            trading: TradingConfig::default(),
            market: MarketConfig::default(),
            polyclob: PolymarketConfig::default(),
            strategy: StrategyConfig::default(),
            edge: EdgeConfigFile::default(),
            risk: RiskConfig::default(),
            polling: PollingConfig::default(),
        }
    }
}

impl Default for TradingConfig {
    fn default() -> Self {
        Self {
            mode: "paper".to_string(),
            private_key: default_private_key(),
        }
    }
}

impl Default for MarketConfig {
    fn default() -> Self {
        Self {
            event_url: String::new(),
            series_id: String::new(),
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
            max_position_size: 50.0,
            min_order_size: 5.0,
            extreme_threshold: 0.80,
            fair_value: 0.50,
            btc_tiebreaker_usd: 5.0,
            momentum_threshold: 0.001,
            momentum_lookback_ms: 120_000,
        }
    }
}

impl Default for EdgeConfigFile {
    fn default() -> Self {
        Self {
            edge_threshold_early: 0.15,
            edge_threshold_mid: 0.15,
            edge_threshold_late: 0.20,
            min_prob_early: 0.50,
            min_prob_mid: 0.50,
            min_prob_late: 0.50,
        }
    }
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_daily_loss_usdc: 100.0,
            max_consecutive_losses: 8,
            max_daily_loss_pct: 0.10,
            cooldown_ms: 5_000,
            max_risk_fraction: 0.10,
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

// ─── Market helpers ───

impl MarketConfig {
    pub fn extract_event_slug(url: &str) -> Option<String> {
        url.trim_end_matches('/')
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    }

    pub fn extract_series_from_slug(slug: &str) -> Option<String> {
        let parts: Vec<&str> = slug.split('-').collect();
        if parts.len() < 2 {
            return Some(slug.to_string());
        }
        let last = parts.last().unwrap();
        if last.chars().all(|c| c.is_ascii_digit()) && last.len() >= 8 {
            Some(parts[..parts.len() - 1].join("-"))
        } else {
            Some(slug.to_string())
        }
    }

    pub fn resolve_series_id(&self) -> String {
        if !self.event_url.is_empty() {
            if let Some(slug) = Self::extract_event_slug(&self.event_url) {
                if let Some(series) = Self::extract_series_from_slug(&slug) {
                    return series;
                }
            }
        }
        self.series_id.clone()
    }
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        let mut config: Config = serde_json::from_str(&content)?;
        // Load private key from env (not stored in config.json)
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
        if self.polling.signal_interval_ms == 0 {
            anyhow::bail!("polling.signal_interval_ms must be > 0");
        }
        if self.strategy.max_position_size <= 0.0 {
            anyhow::bail!("strategy.max_position_size must be > 0");
        }
        if self.strategy.min_order_size <= 0.0 {
            anyhow::bail!("strategy.min_order_size must be > 0");
        }
        if self.strategy.min_order_size > self.strategy.max_position_size {
            anyhow::bail!("strategy.min_order_size must be <= strategy.max_position_size");
        }
        if !(0.0 < self.strategy.extreme_threshold && self.strategy.extreme_threshold < 1.0) {
            anyhow::bail!("strategy.extreme_threshold must be in (0, 1)");
        }
        if !(0.0 < self.strategy.fair_value && self.strategy.fair_value < 1.0) {
            anyhow::bail!("strategy.fair_value must be in (0, 1)");
        }
        if !(0.0 < self.risk.max_risk_fraction && self.risk.max_risk_fraction <= 1.0) {
            anyhow::bail!("risk.max_risk_fraction must be in (0, 1]");
        }
        if !(0.0 < self.risk.max_daily_loss_pct && self.risk.max_daily_loss_pct <= 1.0) {
            anyhow::bail!("risk.max_daily_loss_pct must be in (0, 1]");
        }
        if self.market.window_minutes <= 0.0 {
            anyhow::bail!("market.window_minutes must be > 0");
        }

        Ok(())
    }

    pub fn is_default_non_trading(&self) -> bool {
        let defaults = Config::default();

        self.market.event_url == defaults.market.event_url
            && self.market.series_id == defaults.market.series_id
            && self.market.window_minutes == defaults.market.window_minutes
            && self.polyclob.gamma_api_url == defaults.polyclob.gamma_api_url
            && self.strategy.max_position_size == defaults.strategy.max_position_size
            && self.strategy.min_order_size == defaults.strategy.min_order_size
            && self.strategy.extreme_threshold == defaults.strategy.extreme_threshold
            && self.strategy.fair_value == defaults.strategy.fair_value
            && self.strategy.btc_tiebreaker_usd == defaults.strategy.btc_tiebreaker_usd
            && self.strategy.momentum_threshold == defaults.strategy.momentum_threshold
            && self.strategy.momentum_lookback_ms == defaults.strategy.momentum_lookback_ms
            && self.edge.edge_threshold_early == defaults.edge.edge_threshold_early
            && self.edge.edge_threshold_mid == defaults.edge.edge_threshold_mid
            && self.edge.edge_threshold_late == defaults.edge.edge_threshold_late
            && self.edge.min_prob_early == defaults.edge.min_prob_early
            && self.edge.min_prob_mid == defaults.edge.min_prob_mid
            && self.edge.min_prob_late == defaults.edge.min_prob_late
            && self.risk.max_daily_loss_usdc == defaults.risk.max_daily_loss_usdc
            && self.risk.max_consecutive_losses == defaults.risk.max_consecutive_losses
            && self.risk.max_daily_loss_pct == defaults.risk.max_daily_loss_pct
            && self.risk.cooldown_ms == defaults.risk.cooldown_ms
            && self.risk.max_risk_fraction == defaults.risk.max_risk_fraction
            && self.polling.signal_interval_ms == defaults.polling.signal_interval_ms
    }
}

// ─── Tests ───

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_event_slug() {
        assert_eq!(
            MarketConfig::extract_event_slug(
                "https://polymarket.com/event/btc-updown-5m-1773364500"
            ),
            Some("btc-updown-5m-1773364500".to_string())
        );
        assert_eq!(MarketConfig::extract_event_slug(""), None);
    }

    #[test]
    fn test_extract_series_from_slug() {
        assert_eq!(
            MarketConfig::extract_series_from_slug("btc-updown-5m-1773364500"),
            Some("btc-updown-5m".to_string())
        );
        assert_eq!(
            MarketConfig::extract_series_from_slug("btc-updown-5m"),
            Some("btc-updown-5m".to_string())
        );
    }

    #[test]
    fn test_resolve_series_id() {
        let mut cfg = MarketConfig::default();
        cfg.event_url = "https://polymarket.com/event/btc-updown-5m-1773364500".to_string();
        assert_eq!(cfg.resolve_series_id(), "btc-updown-5m");
    }

    #[test]
    fn test_validate_rejects_zero_interval() {
        let mut cfg = Config::default();
        cfg.polling.signal_interval_ms = 0;

        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_min_greater_than_max() {
        let mut cfg = Config::default();
        cfg.strategy.min_order_size = 20.0;
        cfg.strategy.max_position_size = 10.0;

        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_validate_accepts_defaults() {
        let cfg = Config::default();

        assert!(cfg.validate().is_ok());
    }
}
