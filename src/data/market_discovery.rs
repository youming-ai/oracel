//! Market Auto-Discovery
//!
//! Automatically discovers and tracks the current active 5-minute BTC market
//! on Polymarket. Handles market rotation (new market every 5 minutes).
//! 
//! Uses direct slug lookup: btc-updown-5m-{timestamp} where timestamp is
//! a Unix timestamp rounded to 5-minute boundaries.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ─── Gamma API Types ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GammaMarket {
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub question: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(rename = "endDate", default)]
    pub end_date: String,
    #[serde(rename = "eventStartTime", default)]
    pub event_start_time: Option<String>,
    #[serde(rename = "clobTokenIds", default)]
    pub clob_token_ids: Option<serde_json::Value>,
    #[serde(rename = "conditionId", default)]
    pub condition_id: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ActiveMarket {
    pub market: GammaMarket,
    pub token_id_yes: String,
    pub token_id_no: String,
    pub condition_id: String,
    pub price_to_beat: Option<f64>,
    pub end_date: DateTime<Utc>,
    pub fetched_at: DateTime<Utc>,
}

// ─── Discovery Config ───

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DiscoveryConfig {
    pub series_id: String,
    pub gamma_api_url: String,
    pub refresh_interval_sec: u64,
    pub window_minutes: f64,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            series_id: String::new(),
            gamma_api_url: "https://gamma-api.polymarket.com".to_string(),
            refresh_interval_sec: 60,
            window_minutes: 5.0,
        }
    }
}

// ─── Market Discovery ───

pub struct MarketDiscovery {
    config: DiscoveryConfig,
    client: reqwest::Client,
}

impl MarketDiscovery {
    pub fn new(config: DiscoveryConfig) -> Self {
        Self { config, client: reqwest::Client::new() }
    }

    /// Generate slug for a given timestamp window
    /// e.g., "btc-updown-5m-1773742200"
    fn generate_slug(series: &str, window_ts: i64) -> String {
        format!("{}-{}", series, window_ts)
    }

    /// Calculate the current 5-minute window timestamp
    fn current_window_ts() -> i64 {
        let now = chrono::Utc::now().timestamp();
        // Round DOWN to nearest 5-minute boundary
        (now / 300) * 300
    }

    /// Find the next active market by searching slug patterns
    pub async fn discover(&self) -> Result<ActiveMarket> {
        let base = &self.config.gamma_api_url;
        let window_ts = Self::current_window_ts();
        
        // Search nearby windows (current + next few)
        for offset in 0..5 {
            let ts = window_ts + offset * 300;
            let slug = Self::generate_slug(&self.config.series_id, ts);
            
            let url = format!("{}/events?slug={}&limit=1", base, slug);
            if let Ok(resp) = self.client.get(&url).send().await {
                if resp.status().is_success() {
                    if let Ok(data) = resp.json::<serde_json::Value>().await {
                        if let Some(events) = data.as_array() {
                            if let Some(event) = events.first() {
                                if let Some(markets) = event.get("markets").and_then(|m| m.as_array()) {
                                    for market_json in markets {
                                        if let Ok(market) = serde_json::from_value::<GammaMarket>(market_json.clone()) {
                                            if let Ok(active) = Self::parse_active_market(&market) {
                                                tracing::info!("[MKT] found {} ends {}", slug, active.end_date);
                                                return Ok(active);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        anyhow::bail!("No active market found for series: {}", self.config.series_id)
    }

    /// Parse a GammaMarket into an ActiveMarket
    fn parse_active_market(market: &GammaMarket) -> Result<ActiveMarket> {
        let token_ids = parse_string_array(&market.clob_token_ids);
        let token_id_yes = token_ids
            .first()
            .ok_or_else(|| anyhow::anyhow!("No token_id_yes found"))?
            .clone();
        let token_id_no = token_ids
            .get(1)
            .ok_or_else(|| anyhow::anyhow!("No token_id_no found"))?
            .clone();

        let condition_id = market.condition_id.clone().unwrap_or_default();
        let end_date = parse_datetime(&market.end_date)
            .context("Failed to parse end_date")?;

        let price_to_beat = market
            .question
            .as_ref()
            .or(market.title.as_ref())
            .and_then(|q| extract_price_to_beat(q));

        Ok(ActiveMarket {
            market: market.clone(),
            token_id_yes,
            token_id_no,
            condition_id,
            price_to_beat,
            end_date,
            fetched_at: Utc::now(),
        })
    }
}

// ─── Helpers ───

fn parse_string_array(value: &Option<serde_json::Value>) -> Vec<String> {
    match value {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        Some(serde_json::Value::String(s)) => {
            serde_json::from_str::<Vec<String>>(s).unwrap_or_else(|_| vec![s.clone()])
        }
        _ => Vec::new(),
    }
}

fn parse_datetime(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .map(|nd| nd.and_utc())
        })
}

fn extract_price_to_beat(text: &str) -> Option<f64> {
    for (i, _) in text.match_indices('$') {
        let after = text[i + 1..].trim_start();
        let num_str: String = after
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == ',' || *c == '.')
            .collect();
        if !num_str.is_empty() {
            let cleaned = num_str.replace(',', "");
            if let Ok(price) = cleaned.parse::<f64>() {
                if price > 0.0 {
                    return Some(price);
                }
            }
        }
    }
    None
}

// ─── Tests ───

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_slug() {
        assert_eq!(
            MarketDiscovery::generate_slug("btc-updown-5m", 1773742200),
            "btc-updown-5m-1773742200"
        );
    }

    #[test]
    fn test_parse_string_array() {
        assert_eq!(
            parse_string_array(&Some(serde_json::json!(["t1", "t2"]))),
            vec!["t1", "t2"]
        );
    }
}
