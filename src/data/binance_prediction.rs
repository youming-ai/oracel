//! Official Binance Web3 Wallet Prediction Trading API client.

use std::collections::HashMap;

use anyhow::{Context, Result};
use binance_sdk::{
    config::ConfigurationRestApi,
    w3w_prediction::{
        rest_api::{
            ListPredictionWalletsParams, MarketSearchParams, QueryPaymentOptionBalancesParams,
            RestApi,
        },
        W3WPredictionRestApi,
    },
};
use rust_decimal::Decimal;
use serde::Serialize;

const REST_BASE_URL: &str = "https://api.binance.com";

#[derive(Debug, Clone, Serialize)]
pub struct PredictionWallet {
    pub wallet_id: String,
    pub wallet_address: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PaymentBalance {
    pub account_type: String,
    pub available: Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct PredictionMarketSummary {
    pub market_topic_id: i64,
    pub vendor: String,
    pub slug: String,
    pub title: String,
    pub symbol: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccessReport {
    pub wallets: Vec<PredictionWallet>,
    pub balances: Vec<PaymentBalance>,
    pub btc_5m_candidates: Vec<PredictionMarketSummary>,
}

pub struct BinancePredictionClient {
    api: RestApi,
}

impl BinancePredictionClient {
    pub fn new(api_key: String, api_secret: String) -> Result<Self> {
        if api_key.trim().is_empty() || api_secret.trim().is_empty() {
            anyhow::bail!("BINANCE_API_KEY and BINANCE_API_SECRET are required");
        }
        let config = ConfigurationRestApi::builder()
            .api_key(api_key)
            .api_secret(api_secret)
            .base_path(REST_BASE_URL.to_string())
            .timeout(10_000)
            .keep_alive(true)
            .retries(1)
            .build()
            .context("failed to configure Binance Prediction API")?;
        Ok(Self {
            api: W3WPredictionRestApi::from_config(config),
        })
    }

    pub fn from_env() -> Result<Self> {
        let api_key =
            std::env::var("BINANCE_API_KEY").context("BINANCE_API_KEY is not set in .env")?;
        let api_secret =
            std::env::var("BINANCE_API_SECRET").context("BINANCE_API_SECRET is not set in .env")?;
        Self::new(api_key, api_secret)
    }

    pub async fn access_report(&self) -> Result<AccessReport> {
        let wallets = self.list_wallets().await?;
        let balances = self.list_balances().await?;
        let btc_5m_candidates = self.search_btc_5m().await?;
        Ok(AccessReport {
            wallets,
            balances,
            btc_5m_candidates,
        })
    }

    pub async fn list_wallets(&self) -> Result<Vec<PredictionWallet>> {
        let response = self
            .api
            .list_prediction_wallets(ListPredictionWalletsParams::builder().build()?)
            .await
            .context("Binance Prediction wallet request failed")?
            .data()
            .await
            .context("Binance Prediction wallet response failed")?;
        response
            .wallets
            .unwrap_or_default()
            .into_iter()
            .map(|wallet| {
                Ok(PredictionWallet {
                    wallet_id: wallet
                        .wallet_id
                        .context("wallet response omitted walletId")?,
                    wallet_address: wallet
                        .wallet_address
                        .context("wallet response omitted walletAddress")?,
                })
            })
            .collect()
    }

    pub async fn list_balances(&self) -> Result<Vec<PaymentBalance>> {
        let response = self
            .api
            .query_payment_option_balances(QueryPaymentOptionBalancesParams::builder().build()?)
            .await
            .context("Binance Prediction balance request failed")?
            .data()
            .await
            .context("Binance Prediction balance response failed")?;
        response
            .items
            .unwrap_or_default()
            .into_iter()
            .filter(|item| item.enabled.unwrap_or(false))
            .map(|item| {
                Ok(PaymentBalance {
                    account_type: item.account_type.unwrap_or_else(|| "UNKNOWN".to_string()),
                    available: item
                        .available_balance_display
                        .context("balance response omitted availableBalanceDisplay")?
                        .parse()
                        .context("invalid prediction balance")?,
                })
            })
            .collect()
    }

    pub async fn search_btc_5m(&self) -> Result<Vec<PredictionMarketSummary>> {
        let mut candidates = HashMap::new();
        for query in ["BTC 5m", "Bitcoin 5 minute"] {
            let response = self
                .api
                .market_search(
                    MarketSearchParams::builder(query.to_string())
                        .top_k(50)
                        .build()?,
                )
                .await
                .with_context(|| format!("Binance Prediction market search failed for {query}"))?
                .data()
                .await
                .context("Binance Prediction market search response failed")?;
            for market in response {
                let Some(topic_id) = market.market_topic_id else {
                    continue;
                };
                let symbol = market.symbol.unwrap_or_default();
                let start_ms = market.start_date.unwrap_or_default();
                let end_ms = market.end_date.unwrap_or_default();
                let duration_ms = end_ms.saturating_sub(start_ms);
                let text = format!(
                    "{} {} {}",
                    market.title.as_deref().unwrap_or_default(),
                    market.question.as_deref().unwrap_or_default(),
                    market.description.as_deref().unwrap_or_default()
                )
                .to_ascii_lowercase();
                let is_btc = symbol.eq_ignore_ascii_case("BTCUSDT") || text.contains("btc");
                let is_five_minutes = (240_000..=360_000).contains(&duration_ms)
                    || text.contains("5 min")
                    || text.contains("5-minute")
                    || text.contains("5 minute");
                if !is_btc || !is_five_minutes {
                    continue;
                }
                candidates.insert(
                    topic_id,
                    PredictionMarketSummary {
                        market_topic_id: topic_id,
                        vendor: market.vendor.unwrap_or_default(),
                        slug: market.slug.unwrap_or_default(),
                        title: market.title.unwrap_or_default(),
                        symbol,
                        start_ms,
                        end_ms,
                        status: market.status.unwrap_or_default(),
                    },
                );
            }
        }
        let mut candidates: Vec<_> = candidates.into_values().collect();
        candidates.sort_by_key(|market| market.start_ms);
        Ok(candidates)
    }
}
