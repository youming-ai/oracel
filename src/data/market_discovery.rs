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

use crate::pipeline::signal::Direction;

const WINDOW_SECS: i64 = 300;

// ─── Gamma API Types ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct GammaMarket {
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
    #[serde(default)]
    pub closed: Option<bool>,
    #[serde(rename = "umaResolutionStatus", default)]
    pub uma_resolution_status: Option<String>,
    #[serde(default)]
    pub outcomes: Option<String>,
    #[serde(rename = "outcomePrices", default)]
    pub outcome_prices: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolutionState {
    Pending,
    Resolved(Direction),
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveMarket {
    pub market: GammaMarket,
    pub token_id_yes: String,
    pub token_id_no: String,
    pub condition_id: String,
    pub end_date: DateTime<Utc>,
}

// ─── Discovery Config ───

#[derive(Debug, Clone)]
pub(crate) struct DiscoveryConfig {
    pub series_id: String,
    pub gamma_api_url: String,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            series_id: String::new(),
            gamma_api_url: "https://gamma-api.polymarket.com".to_string(),
        }
    }
}

// ─── Market Discovery ───

pub(crate) struct MarketDiscovery {
    config: DiscoveryConfig,
    client: reqwest::Client,
}

impl MarketDiscovery {
    pub(crate) fn new(config: DiscoveryConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { config, client }
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
        (now / WINDOW_SECS) * WINDOW_SECS
    }

    /// Find the next active market by searching slug patterns
    pub(crate) async fn discover(&self) -> Result<ActiveMarket> {
        let base = &self.config.gamma_api_url;
        let window_ts = Self::current_window_ts();

        // Search nearby windows (current + next few)
        for offset in 0..5 {
            let ts = window_ts + offset * WINDOW_SECS;
            let slug = Self::generate_slug(&self.config.series_id, ts);

            let url = format!("{}/events?slug={}&limit=1", base, slug);

            // HTTP request
            let resp = match self.client.get(&url).send().await {
                Ok(r) => r,
                Err(e) => {
                    tracing::debug!("[MKT] {} request failed: {}", slug, e);
                    continue;
                }
            };

            // Status check
            if !resp.status().is_success() {
                tracing::debug!("[MKT] {} status {}", slug, resp.status());
                continue;
            }

            // JSON parse
            let data: serde_json::Value = match resp.json().await {
                Ok(d) => d,
                Err(e) => {
                    tracing::debug!("[MKT] {} parse failed: {}", slug, e);
                    continue;
                }
            };

            if let Some(events) = data.as_array() {
                if let Some(event) = events.first() {
                    if let Some(markets) = event.get("markets").and_then(|m| m.as_array()) {
                        for market_json in markets {
                            if let Ok(market) =
                                serde_json::from_value::<GammaMarket>(market_json.clone())
                            {
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

        anyhow::bail!(
            "No active market found for series: {}",
            self.config.series_id
        )
    }

    pub(crate) async fn fetch_market_by_slug(&self, slug: &str) -> Result<GammaMarket> {
        let url = format!("{}/markets/slug/{}", self.config.gamma_api_url, slug);
        let market = self
            .client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("Gamma request failed for slug {}", slug))?
            .error_for_status()
            .with_context(|| format!("Gamma returned non-success for slug {}", slug))?
            .json::<GammaMarket>()
            .await
            .with_context(|| format!("Gamma parse failed for slug {}", slug))?;
        Ok(market)
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
        let end_date = parse_datetime(&market.end_date).context("Failed to parse end_date")?;

        Ok(ActiveMarket {
            market: market.clone(),
            token_id_yes,
            token_id_no,
            condition_id,
            end_date,
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

fn parse_json_string_array(value: &Option<String>) -> Option<Vec<String>> {
    let raw = value.as_ref()?;
    serde_json::from_str::<Vec<String>>(raw).ok()
}

pub(crate) fn infer_resolution_state(market: &GammaMarket) -> Option<ResolutionState> {
    let status = market
        .uma_resolution_status
        .as_deref()?
        .to_ascii_lowercase();
    if !status.contains("resolved") {
        return Some(ResolutionState::Pending);
    }

    if market.closed != Some(true) {
        return Some(ResolutionState::Pending);
    }

    let outcomes = parse_json_string_array(&market.outcomes)?;
    let prices = parse_json_string_array(&market.outcome_prices)?;
    if outcomes.len() != prices.len() {
        return None;
    }

    for (outcome, price) in outcomes.iter().zip(prices.iter()) {
        let parsed = price.parse::<f64>().ok()?;
        let normalized = outcome.to_ascii_lowercase();
        if parsed >= 0.999 {
            if normalized == "yes" {
                return Some(ResolutionState::Resolved(Direction::Up));
            }
            if normalized == "no" {
                return Some(ResolutionState::Resolved(Direction::Down));
            }
        }
    }

    None
}

// ─── Tests ───

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::signal::Direction;

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

    #[test]
    fn test_infer_resolved_direction_yes_wins() {
        let market = GammaMarket {
            slug: "btc-updown-5m-1".into(),
            question: None,
            title: None,
            end_date: String::new(),
            event_start_time: None,
            clob_token_ids: None,
            condition_id: None,
            closed: Some(true),
            uma_resolution_status: Some("resolved".into()),
            outcomes: Some("[\"Yes\",\"No\"]".into()),
            outcome_prices: Some("[\"1\",\"0\"]".into()),
        };

        assert_eq!(
            infer_resolution_state(&market),
            Some(ResolutionState::Resolved(Direction::Up))
        );
    }

    #[test]
    fn test_infer_resolved_direction_no_wins() {
        let market = GammaMarket {
            slug: "btc-updown-5m-1".into(),
            question: None,
            title: None,
            end_date: String::new(),
            event_start_time: None,
            clob_token_ids: None,
            condition_id: None,
            closed: Some(true),
            uma_resolution_status: Some("resolved".into()),
            outcomes: Some("[\"Yes\",\"No\"]".into()),
            outcome_prices: Some("[\"0\",\"1\"]".into()),
        };

        assert_eq!(
            infer_resolution_state(&market),
            Some(ResolutionState::Resolved(Direction::Down))
        );
    }

    #[test]
    fn test_infer_resolved_direction_none_when_unresolved() {
        let market = GammaMarket {
            slug: "btc-updown-5m-1".into(),
            question: None,
            title: None,
            end_date: String::new(),
            event_start_time: None,
            clob_token_ids: None,
            condition_id: None,
            closed: Some(false),
            uma_resolution_status: Some("pending".into()),
            outcomes: Some("[\"Yes\",\"No\"]".into()),
            outcome_prices: Some("[\"1\",\"0\"]".into()),
        };

        assert_eq!(
            infer_resolution_state(&market),
            Some(ResolutionState::Pending)
        );
    }
}
