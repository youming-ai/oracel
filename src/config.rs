//! Bot configuration

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
    pub private_key: String,
}

// ─── Market ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketConfig {
    #[serde(default)]
    pub event_url: String,
    pub series_id: String,
    pub token_id_yes: String,
    pub token_id_no: String,
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
}

fn default_max_daily_loss_pct() -> f64 {
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
            private_key: String::new(),
        }
    }
}

impl Default for MarketConfig {
    fn default() -> Self {
        Self {
            event_url: String::new(),
            series_id: String::new(),
            token_id_yes: String::new(),
            token_id_no: String::new(),
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
        }
    }
}

impl Default for PollingConfig {
    fn default() -> Self {
        Self {
            signal_interval_ms: 2000,
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
        let config: Config = serde_json::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
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
}
