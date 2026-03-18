//! Bot configuration

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
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
    #[serde(default = "default_trading_mode", skip_serializing)]
    pub mode: String,
    /// Loaded from PRIVATE_KEY env var (not stored in config.json)
    #[serde(skip)]
    pub private_key: String,
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
fn default_trading_mode() -> String {
    "paper".to_string()
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
            mode: default_trading_mode(),
            private_key: String::new(),
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
        if let Some(mode) = std::env::var("TRADING_MODE")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            config.trading.mode = mode;
        }
        // Load private key from env (not stored in config.json)
        if let Ok(pk) = std::env::var("PRIVATE_KEY") {
            config.trading.private_key = pk;
        }
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
    use std::env;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        env_lock().lock().unwrap_or_else(|err| err.into_inner())
    }

    fn write_test_config(path: &Path) {
        let content = r#"{
  "trading": {},
  "market": {
    "event_url": "",
    "series_id": "btc-updown-5m",
    "window_minutes": 5.0
  },
  "polyclob": {
    "gamma_api_url": "https://gamma-api.polymarket.com"
  },
  "strategy": {
    "max_position_size": 5.0,
    "min_order_size": 1.0
  },
  "edge": {
    "edge_threshold_early": 0.15
  },
  "risk": {
    "max_daily_loss_usdc": 10.0,
    "max_consecutive_losses": 5
  },
  "polling": {
    "signal_interval_ms": 1000
  }
}"#;
        fs::write(path, content).expect("write config fixture");
    }

    fn write_test_config_without_trading(path: &Path) {
        let content = r#"{
  "market": {
    "event_url": "",
    "series_id": "btc-updown-5m",
    "window_minutes": 5.0
  },
  "polyclob": {
    "gamma_api_url": "https://gamma-api.polymarket.com"
  },
  "strategy": {
    "max_position_size": 5.0,
    "min_order_size": 1.0
  },
  "edge": {
    "edge_threshold_early": 0.15
  },
  "risk": {
    "max_daily_loss_usdc": 10.0,
    "max_consecutive_losses": 5
  },
  "polling": {
    "signal_interval_ms": 1000
  }
}"#;
        fs::write(path, content).expect("write config fixture without trading");
    }

    fn write_test_config_with_live_mode(path: &Path) {
        let content = r#"{
  "trading": {
    "mode": "live"
  },
  "market": {
    "event_url": "",
    "series_id": "btc-updown-5m",
    "window_minutes": 5.0
  },
  "polyclob": {
    "gamma_api_url": "https://gamma-api.polymarket.com"
  },
  "strategy": {
    "max_position_size": 5.0,
    "min_order_size": 1.0
  },
  "edge": {
    "edge_threshold_early": 0.15
  },
  "risk": {
    "max_daily_loss_usdc": 10.0,
    "max_consecutive_losses": 5
  },
  "polling": {
    "signal_interval_ms": 1000
  }
}"#;
        fs::write(path, content).expect("write config fixture with live mode");
    }

    fn test_config_path(test_name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("oracel-{test_name}-{}.json", std::process::id()));
        path
    }

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
    fn test_load_reads_trading_mode_from_env() {
        let _guard = lock_env();
        let path = test_config_path("trading-mode-env");
        write_test_config(&path);
        env::set_var("TRADING_MODE", "live");

        let config = Config::load(&path).expect("load config");

        assert_eq!(config.trading.mode, "live");

        env::remove_var("TRADING_MODE");
        fs::remove_file(path).expect("remove config fixture");
    }

    #[test]
    fn test_load_defaults_trading_mode_to_paper() {
        let _guard = lock_env();
        let path = test_config_path("trading-mode-default");
        write_test_config(&path);
        env::remove_var("TRADING_MODE");

        let config = Config::load(&path).expect("load config");

        assert_eq!(config.trading.mode, "paper");

        fs::remove_file(path).expect("remove config fixture");
    }

    #[test]
    fn test_load_defaults_trading_when_section_missing() {
        let _guard = lock_env();
        let path = test_config_path("trading-missing-section");
        write_test_config_without_trading(&path);
        env::remove_var("TRADING_MODE");

        let config = Config::load(&path).expect("load config");

        assert_eq!(config.trading.mode, "paper");
        assert_eq!(config.trading.private_key, "");

        fs::remove_file(path).expect("remove config fixture");
    }

    #[test]
    fn test_load_uses_legacy_trading_mode_when_env_unset() {
        let _guard = lock_env();
        let path = test_config_path("trading-legacy-live");
        write_test_config_with_live_mode(&path);
        env::remove_var("TRADING_MODE");

        let config = Config::load(&path).expect("load config");

        assert_eq!(config.trading.mode, "live");

        fs::remove_file(path).expect("remove config fixture");
    }

    #[test]
    fn test_load_ignores_empty_trading_mode_env() {
        let _guard = lock_env();
        let path = test_config_path("trading-empty-env");
        write_test_config_with_live_mode(&path);
        env::set_var("TRADING_MODE", "   ");

        let config = Config::load(&path).expect("load config");

        assert_eq!(config.trading.mode, "live");

        env::remove_var("TRADING_MODE");
        fs::remove_file(path).expect("remove config fixture");
    }
}
