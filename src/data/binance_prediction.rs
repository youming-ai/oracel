//! Binance Web3 Wallet Prediction Trading API integration.
//!
//! All market discovery, order-book access, execution, position reconciliation,
//! settlement, and redemption use Binance's official Prediction REST API.

use std::collections::{BTreeSet, HashMap};

use anyhow::{Context, Result};
use binance_sdk::{
    config::ConfigurationRestApi,
    w3w_prediction::{
        rest_api::{
            GetMarketDetailParams, ListPredictionMarketsOrderByEnum, ListPredictionMarketsParams,
            ListPredictionMarketsSortByEnum, ListPredictionWalletsParams, MarketSearchParams,
            QueryOrderBookParams, QueryOrderHistoryParams, QueryPaymentOptionBalancesParams,
            QuerySettledPositionHistoryParams, RestApi,
        },
        W3WPredictionRestApi,
    },
};
use chrono::{Duration, Utc};
use hmac::{Hmac, Mac};
use reqwest::header::HeaderValue;
use rust_decimal::Decimal;
use secrecy::ExposeSecret;
use serde::de::DeserializeOwned;
use sha2::Sha256;
use url::form_urlencoded;

use crate::config::{BinancePredictionConfig, FundingSource, PaymentAccount};
use crate::pipeline::decider::Direction;

const REST_BASE_URL: &str = "https://api.binance.com";
const WEI_PER_USDT: &str = "1000000000000000000";
const MARKET_DURATION_TOLERANCE_MS: u64 = 60_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PredictionWallet {
    pub wallet_id: String,
    pub wallet_address: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BuyQuote {
    pub best_bid: Option<Decimal>,
    pub best_ask: Decimal,
    pub spread: Option<Decimal>,
    pub best_ask_notional: Decimal,
    pub effective_price: Decimal,
    pub limit_price: Decimal,
    pub timestamp_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketToken {
    pub market_id: i64,
    pub token_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActivePredictionMarket {
    pub market_topic_id: i64,
    pub vendor: String,
    pub slug: String,
    pub title: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub reference_price: Decimal,
    pub fee_rate_bps: u32,
    pub up: MarketToken,
    pub down: MarketToken,
}

impl ActivePredictionMarket {
    pub fn token(&self, direction: Direction) -> &MarketToken {
        match direction {
            Direction::Up => &self.up,
            Direction::Down => &self.down,
        }
    }

    pub fn key(&self) -> String {
        self.market_topic_id.to_string()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmedFill {
    pub order_id: String,
    pub filled_shares: Decimal,
    pub trade_cost: Decimal,
    pub fee: Decimal,
}

impl ConfirmedFill {
    pub fn total_cost(&self) -> Decimal {
        self.trade_cost + self.fee
    }

    pub fn entry_price(&self) -> Decimal {
        self.trade_cost / self.filled_shares
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum OrderReconciliation {
    Pending,
    Unfilled,
    Filled(ConfirmedFill),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LiveSettlement {
    pub won: bool,
    pub payout: Decimal,
    pub pnl: Decimal,
    pub redeem_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RedeemReceipt {
    pub batch_id: Option<String>,
    pub transaction_hash: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct PredictionRuntimeConfig {
    pub market_symbol: String,
    pub market_duration_ms: u64,
    pub discovery_limit: usize,
    pub quote_slippage_bps: u32,
    pub reconciliation_attempts: u32,
    pub reconciliation_delay_ms: u64,
    pub payment_account: PaymentAccount,
    pub funding_source: FundingSource,
}

impl From<&BinancePredictionConfig> for PredictionRuntimeConfig {
    fn from(config: &BinancePredictionConfig) -> Self {
        Self {
            market_symbol: "BTCUSDT".to_string(),
            market_duration_ms: config.market_duration_ms,
            discovery_limit: config.market_discovery_limit,
            quote_slippage_bps: config.quote_slippage_bps,
            reconciliation_attempts: config.order_reconciliation_attempts,
            reconciliation_delay_ms: config.order_reconciliation_delay_ms,
            payment_account: config.payment_account,
            funding_source: config.funding_source,
        }
    }
}

struct SignedPostClient {
    http: reqwest::Client,
    api_key: String,
    api_secret: String,
}

impl SignedPostClient {
    fn new(api_key: String, api_secret: String, timeout_ms: u64) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(timeout_ms))
            .build()
            .context("failed to build non-retrying Binance trade client")?;
        Ok(Self {
            http,
            api_key,
            api_secret,
        })
    }

    /// Submit one signed POST without an automatic retry. Retrying a failed
    /// transport after an exchange may have accepted an order is unsafe.
    async fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        parameters: Vec<(String, String)>,
    ) -> Result<T> {
        let query = signed_query(&parameters, Utc::now().timestamp_millis(), &self.api_secret)?;
        let response = self
            .http
            .post(format!("{REST_BASE_URL}{path}?{query}"))
            .header(
                "X-MBX-APIKEY",
                HeaderValue::from_str(&self.api_key).context("invalid Binance API key header")?,
            )
            .send()
            .await
            .with_context(|| format!("Binance Prediction POST {path} transport failed"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .context("failed to read Binance Prediction response")?;
        if !status.is_success() {
            anyhow::bail!("Binance Prediction POST {path} failed ({status}): {body}");
        }
        serde_json::from_str(&body)
            .with_context(|| format!("failed to parse Binance Prediction POST {path} response"))
    }
}

pub struct BinancePredictionClient {
    api: RestApi,
    trade: SignedPostClient,
    runtime: PredictionRuntimeConfig,
    wallet: Option<PredictionWallet>,
}

impl BinancePredictionClient {
    pub async fn connect(config: &BinancePredictionConfig, require_wallet: bool) -> Result<Self> {
        let api_key = config.api_key.expose_secret().to_owned();
        let api_secret = config.api_secret.expose_secret().to_owned();
        let rest_config = ConfigurationRestApi::builder()
            .api_key(api_key.clone())
            .api_secret(api_secret.clone())
            .base_path(REST_BASE_URL.to_string())
            .timeout(config.api_timeout_ms)
            .keep_alive(true)
            // GET discovery/history calls may retry; POST execution uses `trade` below.
            .retries(1)
            .build()
            .context("failed to configure Binance Prediction client")?;
        let mut client = Self {
            api: W3WPredictionRestApi::from_config(rest_config),
            trade: SignedPostClient::new(api_key, api_secret, config.api_timeout_ms)?,
            runtime: PredictionRuntimeConfig::from(config),
            wallet: None,
        };
        if require_wallet {
            client.wallet = Some(
                client
                    .select_wallet(
                        config.wallet_id.as_deref(),
                        config.wallet_address.as_deref(),
                    )
                    .await?,
            );
        }
        Ok(client)
    }

    pub fn runtime(&self) -> &PredictionRuntimeConfig {
        &self.runtime
    }

    pub fn wallet(&self) -> Result<&PredictionWallet> {
        self.wallet
            .as_ref()
            .context("Binance Prediction wallet is required for live execution")
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

    async fn select_wallet(
        &self,
        requested_id: Option<&str>,
        requested_address: Option<&str>,
    ) -> Result<PredictionWallet> {
        let wallets = self.list_wallets().await?;
        let selected: Vec<_> = wallets
            .into_iter()
            .filter(|wallet| {
                requested_id.is_none_or(|id| wallet.wallet_id == id)
                    && requested_address
                        .is_none_or(|address| wallet.wallet_address.eq_ignore_ascii_case(address))
            })
            .collect();
        match selected.as_slice() {
            [wallet] => Ok(wallet.clone()),
            [] => anyhow::bail!("no Binance Prediction wallet matches the configured selection"),
            _ => anyhow::bail!(
                "multiple Binance Prediction wallets found; set BINANCE_PREDICTION_WALLET_ID and BINANCE_PREDICTION_WALLET_ADDRESS"
            ),
        }
    }

    pub async fn payment_balance(&self) -> Result<Decimal> {
        let response = self
            .api
            .query_payment_option_balances(QueryPaymentOptionBalancesParams::builder().build()?)
            .await
            .context("Binance Prediction payment-balance request failed")?
            .data()
            .await
            .context("Binance Prediction payment-balance response failed")?;
        let account_type = self.runtime.payment_account.as_api_str();
        let item = response
            .items
            .unwrap_or_default()
            .into_iter()
            .find(|item| {
                item.enabled.unwrap_or(false) && item.account_type.as_deref() == Some(account_type)
            })
            .with_context(|| {
                format!("Binance Prediction payment account {account_type} is not enabled")
            })?;
        parse_decimal(
            item.available_balance_display
                .as_deref()
                .context("payment balance omitted availableBalanceDisplay")?,
            "available payment balance",
        )
    }

    pub async fn discover_active_market(&self, now_ms: i64) -> Result<ActivePredictionMarket> {
        let limit = i32::try_from(self.runtime.discovery_limit.min(100))
            .context("market_discovery_limit exceeds i32")?;
        let listed = self
            .api
            .list_prediction_markets(
                ListPredictionMarketsParams::builder()
                    .sort_by(ListPredictionMarketsSortByEnum::EndDate)
                    .order_by(ListPredictionMarketsOrderByEnum::Asc)
                    .limit(limit)
                    .build()?,
            )
            .await
            .context("Binance Prediction market-list request failed")?
            .data()
            .await
            .context("Binance Prediction market-list response failed")?;

        let mut topic_ids = BTreeSet::new();
        for topic in listed.market_topics.unwrap_or_default() {
            if topic_matches_candidate(
                topic.symbol.as_deref(),
                topic.title.as_deref(),
                topic.question.as_deref(),
                topic.start_date,
                topic.end_date,
                &self.runtime.market_symbol,
                self.runtime.market_duration_ms,
                now_ms,
            ) {
                if let Some(id) = topic.market_topic_id {
                    topic_ids.insert(id);
                }
            }
        }

        // Semantic search catches BTC windows not included in the first sorted page.
        let searched = self
            .api
            .market_search(
                MarketSearchParams::builder("BTC price".to_string())
                    .top_k(50)
                    .build()?,
            )
            .await
            .context("Binance Prediction BTC market search failed")?
            .data()
            .await
            .context("Binance Prediction BTC market search response failed")?;
        for topic in searched {
            if topic_matches_candidate(
                topic.symbol.as_deref(),
                topic.title.as_deref(),
                topic.question.as_deref(),
                topic.start_date,
                topic.end_date,
                &self.runtime.market_symbol,
                self.runtime.market_duration_ms,
                now_ms,
            ) {
                if let Some(id) = topic.market_topic_id {
                    topic_ids.insert(id);
                }
            }
        }

        let mut active = Vec::new();
        for topic_id in topic_ids.into_iter().take(self.runtime.discovery_limit) {
            let detail = self.market_detail(topic_id).await?;
            if let Ok(market) = parse_active_market(
                detail,
                &self.runtime.market_symbol,
                self.runtime.market_duration_ms,
                now_ms,
            ) {
                active.push(market);
            }
        }
        active
            .into_iter()
            .min_by_key(|market| market.end_ms)
            .context("no active Binance Prediction BTC five-minute market found")
    }

    async fn market_detail(
        &self,
        market_topic_id: i64,
    ) -> Result<binance_sdk::w3w_prediction::rest_api::GetMarketDetailResponse> {
        self.api
            .get_market_detail(GetMarketDetailParams::builder(market_topic_id).build()?)
            .await
            .with_context(|| {
                format!("Binance Prediction detail request failed for {market_topic_id}")
            })?
            .data()
            .await
            .with_context(|| {
                format!("Binance Prediction detail response failed for {market_topic_id}")
            })
    }

    pub async fn fetch_buy_quote(
        &self,
        market: &ActivePredictionMarket,
        direction: Direction,
        amount_usdt: Decimal,
    ) -> Result<BuyQuote> {
        let token = market.token(direction);
        let response = self
            .api
            .query_order_book(
                QueryOrderBookParams::builder(
                    market.vendor.clone(),
                    token.market_id,
                    token.token_id.clone(),
                )
                .build()?,
            )
            .await
            .with_context(|| {
                format!(
                    "Binance Prediction order-book request failed for {}",
                    token.token_id
                )
            })?
            .data()
            .await
            .context("Binance Prediction order-book response failed")?;
        if response
            .token_id
            .as_deref()
            .is_some_and(|returned| returned != token.token_id)
        {
            anyhow::bail!("Binance Prediction order book returned a mismatched token ID");
        }
        let bids = response
            .bids
            .unwrap_or_default()
            .into_iter()
            .map(|level| parse_level(level.price.as_deref(), level.size.as_deref(), "bid"))
            .collect::<Result<Vec<_>>>()?;
        let asks = response
            .asks
            .unwrap_or_default()
            .into_iter()
            .map(|level| parse_level(level.price.as_deref(), level.size.as_deref(), "ask"))
            .collect::<Result<Vec<_>>>()?;
        build_buy_quote(
            &bids,
            asks,
            amount_usdt,
            response
                .timestamp
                .unwrap_or_else(|| Utc::now().timestamp_millis()),
        )
    }

    /// Submit a Binance Prediction MARKET/FOK order only after checking the
    /// quote's minimum receipt against the model-derived maximum price.
    pub async fn submit_market_buy(
        &self,
        market: &ActivePredictionMarket,
        direction: Direction,
        amount_usdt: Decimal,
        max_price: Decimal,
    ) -> Result<String> {
        let wallet = self.wallet()?;
        let token = market.token(direction);
        let amount_in = decimal_to_wei(amount_usdt, "order amount")?;
        let quote: binance_sdk::w3w_prediction::rest_api::GetQuoteResponse = self
            .trade
            .post(
                "/sapi/v1/w3w/wallet/prediction/trade/get-quote",
                vec![
                    ("walletAddress".into(), wallet.wallet_address.clone()),
                    ("tokenId".into(), token.token_id.clone()),
                    ("side".into(), "BUY".into()),
                    ("amountIn".into(), amount_in.clone()),
                    ("orderType".into(), "MARKET".into()),
                    (
                        "slippageBps".into(),
                        self.runtime.quote_slippage_bps.to_string(),
                    ),
                    ("feeRateBps".into(), market.fee_rate_bps.to_string()),
                    (
                        "fundingSource".into(),
                        self.runtime.funding_source.as_api_str().into(),
                    ),
                ],
            )
            .await
            .context("Binance Prediction get-quote request failed")?;
        validate_market_quote(
            &quote,
            &wallet.wallet_address,
            &token.token_id,
            &amount_in,
            self.runtime.quote_slippage_bps,
            max_price,
        )?;
        let quote_id = quote
            .quote_id
            .context("Binance Prediction quote omitted quoteId")?;
        let response: binance_sdk::w3w_prediction::rest_api::PlaceOrderResponse = self
            .trade
            .post(
                "/sapi/v1/w3w/wallet/prediction/trade/place-order-bundle",
                vec![
                    ("walletAddress".into(), wallet.wallet_address.clone()),
                    ("walletId".into(), wallet.wallet_id.clone()),
                    ("quoteId".into(), quote_id),
                    ("timeInForce".into(), "FOK".into()),
                    (
                        "accountType".into(),
                        self.runtime.payment_account.as_api_str().into(),
                    ),
                    ("orderType".into(), "MARKET".into()),
                    (
                        "slippageBps".into(),
                        self.runtime.quote_slippage_bps.to_string(),
                    ),
                    (
                        "fundingSource".into(),
                        self.runtime.funding_source.as_api_str().into(),
                    ),
                ],
            )
            .await
            .context("Binance Prediction place-order request failed")?;
        response
            .order_id
            .filter(|id| !id.is_empty())
            .context("Binance Prediction place-order response omitted orderId")
    }

    pub async fn reconcile_order(&self, order_id: &str) -> Result<OrderReconciliation> {
        let wallet = self.wallet()?;
        let today = Utc::now().date_naive();
        let tomorrow = today
            .checked_add_signed(Duration::days(1))
            .context("failed to calculate order-history date")?;
        let response = self
            .api
            .query_order_history(
                QueryOrderHistoryParams::builder(wallet.wallet_address.clone())
                    .start_date(today.to_string())
                    .end_date(tomorrow.to_string())
                    .limit(100)
                    .build()?,
            )
            .await
            .context("Binance Prediction order-history request failed")?
            .data()
            .await
            .context("Binance Prediction order-history response failed")?;
        let order = response
            .orders
            .unwrap_or_default()
            .into_iter()
            .find(|order| order.order_id.as_deref() == Some(order_id));
        let Some(order) = order else {
            return Ok(OrderReconciliation::Pending);
        };
        let status = order.status.unwrap_or_default().to_ascii_uppercase();
        let filled_usdt = order
            .filled_usdt_amount
            .as_deref()
            .map(|value| wei_to_decimal(value, "filled USDT amount"))
            .transpose()?
            .unwrap_or(Decimal::ZERO);
        let filled_shares = order
            .filled_share_qty
            .as_deref()
            .map(|value| wei_to_decimal(value, "filled share quantity"))
            .transpose()?
            .unwrap_or(Decimal::ZERO);
        if filled_usdt > Decimal::ZERO && filled_shares > Decimal::ZERO {
            let market_fee = order
                .market_provider_fee
                .as_deref()
                .map(|value| wei_to_decimal(value, "market provider fee"))
                .transpose()?
                .unwrap_or(Decimal::ZERO);
            let network_fee = order
                .network_fee
                .as_deref()
                .map(|value| wei_to_decimal(value, "network fee"))
                .transpose()?
                .unwrap_or(Decimal::ZERO);
            return Ok(OrderReconciliation::Filled(ConfirmedFill {
                order_id: order_id.to_string(),
                filled_shares,
                trade_cost: filled_usdt,
                fee: market_fee + network_fee,
            }));
        }
        if matches!(
            status.as_str(),
            "REJECTED" | "CANCELED" | "CANCELLED" | "EXPIRED" | "FAILED" | "FOK_FAILED"
        ) {
            Ok(OrderReconciliation::Unfilled)
        } else {
            Ok(OrderReconciliation::Pending)
        }
    }

    /// Resolve a Paper position from Binance's market-detail reference and end
    /// prices. The API's end price is authoritative for this market product.
    pub async fn paper_resolution(
        &self,
        market_topic_id: i64,
        now_ms: i64,
    ) -> Result<Option<Direction>> {
        let detail = self.market_detail(market_topic_id).await?;
        let end_ms = detail.end_date.context("market detail omitted endDate")?;
        if now_ms < end_ms {
            return Ok(None);
        }
        let variant = detail
            .variant_data
            .context("market detail omitted variantData")?;
        let start = parse_decimal(
            variant
                .start_price
                .as_deref()
                .context("market detail omitted startPrice")?,
            "market start price",
        )?;
        let end = match variant
            .end_price
            .as_ref()
            .and_then(|value| value.as_deref())
        {
            Some(value) if !value.is_empty() => parse_decimal(value, "market end price")?,
            _ => return Ok(None),
        };
        Ok(Some(if end >= start {
            Direction::Up
        } else {
            Direction::Down
        }))
    }

    pub async fn live_settlement(
        &self,
        market_topic_id: i64,
        token_id: &str,
    ) -> Result<Option<LiveSettlement>> {
        let wallet = self.wallet()?;
        let today = Utc::now().date_naive();
        let yesterday = today
            .checked_sub_signed(Duration::days(1))
            .context("failed to calculate settlement start date")?;
        let tomorrow = today
            .checked_add_signed(Duration::days(1))
            .context("failed to calculate settlement end date")?;
        let response = self
            .api
            .query_settled_position_history(
                QuerySettledPositionHistoryParams::builder(wallet.wallet_address.clone())
                    .start_date(yesterday.to_string())
                    .end_date(tomorrow.to_string())
                    .limit(100)
                    .build()?,
            )
            .await
            .context("Binance Prediction settled-position request failed")?
            .data()
            .await
            .context("Binance Prediction settled-position response failed")?;
        let position = response
            .positions
            .unwrap_or_default()
            .into_iter()
            .find(|position| {
                position.market_topic_id == Some(market_topic_id)
                    && position.token_id.as_deref() == Some(token_id)
            });
        let Some(position) = position else {
            return Ok(None);
        };
        let won = position
            .is_winner
            .context("settled position omitted isWinner")?;
        let payout = position
            .claim_amount
            .as_deref()
            .map(|value| parse_decimal(value, "settled claim amount"))
            .transpose()?
            .unwrap_or(Decimal::ZERO);
        let pnl = parse_signed_decimal(
            position
                .pnl
                .as_deref()
                .context("settled position omitted pnl")?,
            "settled position pnl",
        )?;
        Ok(Some(LiveSettlement {
            won,
            payout,
            pnl,
            redeem_status: position.redeem_status,
        }))
    }

    pub async fn redeem(&self, token_id: &str) -> Result<RedeemReceipt> {
        let wallet = self.wallet()?;
        let response: binance_sdk::w3w_prediction::rest_api::BatchRedeemResponse = self
            .trade
            .post(
                "/sapi/v1/w3w/wallet/prediction/batch-redeem",
                vec![
                    ("walletAddress".into(), wallet.wallet_address.clone()),
                    ("walletId".into(), wallet.wallet_id.clone()),
                    ("tokenIds".into(), token_id.to_string()),
                ],
            )
            .await
            .context("Binance Prediction redeem request failed")?;
        let result = response
            .results
            .unwrap_or_default()
            .into_iter()
            .next()
            .context("Binance Prediction redeem response omitted result")?;
        if let Some(Some(error)) = result.error {
            anyhow::bail!("Binance Prediction redeem failed: {error}");
        }
        Ok(RedeemReceipt {
            batch_id: response.batch_id,
            transaction_hash: result.tx_hash,
            status: result.status.unwrap_or_else(|| "UNKNOWN".to_string()),
        })
    }
}

fn signed_query(
    parameters: &[(String, String)],
    timestamp_ms: i64,
    api_secret: &str,
) -> Result<String> {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (key, value) in parameters {
        serializer.append_pair(key, value);
    }
    serializer.append_pair("timestamp", &timestamp_ms.to_string());
    let canonical = serializer.finish();
    let mut mac = Hmac::<Sha256>::new_from_slice(api_secret.as_bytes())
        .context("failed to initialize Binance request signature")?;
    mac.update(canonical.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());
    Ok(format!("{canonical}&signature={signature}"))
}

#[allow(clippy::too_many_arguments)]
fn topic_matches_candidate(
    symbol: Option<&str>,
    title: Option<&str>,
    question: Option<&str>,
    start_ms: Option<i64>,
    end_ms: Option<i64>,
    expected_symbol: &str,
    expected_duration_ms: u64,
    now_ms: i64,
) -> bool {
    let text = format!(
        "{} {}",
        title.unwrap_or_default(),
        question.unwrap_or_default()
    )
    .to_ascii_lowercase();
    let symbol_matches = symbol.is_some_and(|value| value.eq_ignore_ascii_case(expected_symbol))
        || text.contains("btc");
    let duration_matches = match (start_ms, end_ms) {
        (Some(start), Some(end)) => {
            let duration = end.saturating_sub(start).unsigned_abs();
            duration.abs_diff(expected_duration_ms) <= MARKET_DURATION_TOLERANCE_MS
        }
        _ => text.contains("5 min") || text.contains("5-minute") || text.contains("5 minute"),
    };
    let time_matches = end_ms.is_none_or(|end| end >= now_ms.saturating_sub(60_000));
    symbol_matches && duration_matches && time_matches
}

fn parse_active_market(
    detail: binance_sdk::w3w_prediction::rest_api::GetMarketDetailResponse,
    expected_symbol: &str,
    expected_duration_ms: u64,
    now_ms: i64,
) -> Result<ActivePredictionMarket> {
    let market_topic_id = detail
        .market_topic_id
        .context("Binance Prediction detail omitted marketTopicId")?;
    let symbol = detail
        .symbol
        .as_deref()
        .context("Binance Prediction detail omitted symbol")?;
    if !symbol.eq_ignore_ascii_case(expected_symbol) {
        anyhow::bail!("Prediction market symbol {symbol} is not {expected_symbol}");
    }
    let start_ms = detail
        .start_date
        .context("market detail omitted startDate")?;
    let end_ms = detail.end_date.context("market detail omitted endDate")?;
    let duration = end_ms.saturating_sub(start_ms).unsigned_abs();
    if duration.abs_diff(expected_duration_ms) > MARKET_DURATION_TOLERANCE_MS {
        anyhow::bail!("Prediction market duration is not the configured five-minute interval");
    }
    if now_ms < start_ms || now_ms >= end_ms {
        anyhow::bail!("Prediction market is not currently in its trading interval");
    }
    let variant = detail
        .variant_data
        .context("market detail omitted variantData")?;
    if !variant
        .r#type
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case("CRYPTO_UP_DOWN"))
    {
        anyhow::bail!("Prediction market is not a CRYPTO_UP_DOWN market");
    }
    if !variant
        .price_feed_provider
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case("BINANCE"))
        || !variant
            .price_feed_symbol
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case(expected_symbol))
    {
        anyhow::bail!("Prediction market does not use the expected Binance price feed");
    }
    let reference_price = parse_decimal(
        variant
            .start_price
            .as_deref()
            .context("market detail omitted startPrice")?,
        "market start price",
    )?;
    let vendor = detail.vendor.context("market detail omitted vendor")?;
    let fee_rate_bps = u32::try_from(detail.fee_rate_bps.unwrap_or(0))
        .context("market detail feeRateBps is negative")?;

    let mut mapped = HashMap::<Direction, MarketToken>::new();
    for market in detail.markets.unwrap_or_default() {
        if !is_open_status(market.trading_status.as_deref()) {
            continue;
        }
        let market_id = market.market_id.context("market detail omitted marketId")?;
        let context = format!(
            "{} {} {}",
            market.title.as_deref().unwrap_or_default(),
            market.question.as_deref().unwrap_or_default(),
            market.description.as_deref().unwrap_or_default()
        );
        let market_direction = direction_from_market_text(&context);
        let outcomes = market.outcomes.unwrap_or_default();
        for outcome in outcomes {
            let token_id = outcome.token_id.context("market outcome omitted tokenId")?;
            let direction = direction_from_outcome(outcome.name.as_deref(), market_direction)
                .with_context(|| {
                    format!(
                        "cannot map Binance Prediction outcome {:?} for market {:?} to UP or DOWN",
                        outcome.name, market.title
                    )
                })?;
            let token = MarketToken {
                market_id,
                token_id,
            };
            if let Some(previous) = mapped.insert(direction, token.clone()) {
                if previous != token {
                    anyhow::bail!("multiple active tokens map to {}", direction.as_str());
                }
            }
        }
    }
    let up = mapped
        .remove(&Direction::Up)
        .context("active UP token is missing")?;
    let down = mapped
        .remove(&Direction::Down)
        .context("active DOWN token is missing")?;
    Ok(ActivePredictionMarket {
        market_topic_id,
        vendor,
        slug: detail.slug.unwrap_or_else(|| market_topic_id.to_string()),
        title: detail.title.unwrap_or_default(),
        start_ms,
        end_ms,
        reference_price,
        fee_rate_bps,
        up,
        down,
    })
}

fn is_open_status(status: Option<&str>) -> bool {
    matches!(
        status.map(|value| value.to_ascii_uppercase()).as_deref(),
        Some("OPEN" | "ACTIVE" | "TRADING")
    )
}

fn direction_from_market_text(text: &str) -> Option<Direction> {
    let words: Vec<_> = text
        .split(|character: char| !character.is_ascii_alphabetic())
        .map(str::to_ascii_lowercase)
        .collect();
    let up = words.iter().any(|word| {
        matches!(
            word.as_str(),
            "up" | "higher" | "above" | "increase" | "rise"
        )
    });
    let down = words.iter().any(|word| {
        matches!(
            word.as_str(),
            "down" | "lower" | "below" | "decrease" | "fall"
        )
    });
    match (up, down) {
        (true, false) => Some(Direction::Up),
        (false, true) => Some(Direction::Down),
        _ => None,
    }
}

fn direction_from_outcome(
    name: Option<&str>,
    market_direction: Option<Direction>,
) -> Option<Direction> {
    let name = name?.trim().to_ascii_lowercase();
    match name.as_str() {
        "yes" => market_direction,
        "no" => market_direction.map(reverse),
        "up" | "higher" | "above" => Some(Direction::Up),
        "down" | "lower" | "below" => Some(Direction::Down),
        _ => direction_from_market_text(&name),
    }
}

fn reverse(direction: Direction) -> Direction {
    match direction {
        Direction::Up => Direction::Down,
        Direction::Down => Direction::Up,
    }
}

fn parse_decimal(value: &str, label: &str) -> Result<Decimal> {
    let value = parse_signed_decimal(value, label)?;
    if value < Decimal::ZERO {
        anyhow::bail!("Binance Prediction {label} must not be negative");
    }
    Ok(value)
}

fn parse_signed_decimal(value: &str, label: &str) -> Result<Decimal> {
    Decimal::from_str_exact(value)
        .with_context(|| format!("Binance Prediction {label} is not a decimal"))
}

fn parse_level(price: Option<&str>, size: Option<&str>, side: &str) -> Result<(Decimal, Decimal)> {
    let price = parse_decimal(
        price.with_context(|| format!("Binance Prediction {side} is missing price"))?,
        &format!("{side} price"),
    )?;
    let size = parse_decimal(
        size.with_context(|| format!("Binance Prediction {side} is missing size"))?,
        &format!("{side} size"),
    )?;
    if !(Decimal::ZERO..=Decimal::ONE).contains(&price) || size <= Decimal::ZERO {
        anyhow::bail!("Binance Prediction {side} level is out of range");
    }
    Ok((price, size))
}

fn build_buy_quote(
    bids: &[(Decimal, Decimal)],
    mut asks: Vec<(Decimal, Decimal)>,
    amount_usdt: Decimal,
    timestamp_ms: i64,
) -> Result<BuyQuote> {
    if amount_usdt <= Decimal::ZERO {
        anyhow::bail!("order amount must be positive");
    }
    asks.sort_by_key(|(price, _)| *price);
    let best_ask = asks
        .first()
        .map(|(price, _)| *price)
        .context("Binance Prediction order book has no asks")?;
    let best_bid = bids.iter().map(|(price, _)| *price).max();
    let best_ask_notional: Decimal = asks
        .iter()
        .take_while(|(price, _)| *price == best_ask)
        .map(|(price, size)| *price * *size)
        .sum();

    let mut remaining = amount_usdt;
    let mut shares = Decimal::ZERO;
    let mut limit_price = best_ask;
    for (price, size) in asks {
        if remaining <= Decimal::ZERO {
            break;
        }
        let level_notional = price * size;
        let spent = remaining.min(level_notional);
        shares += spent / price;
        remaining -= spent;
        limit_price = price;
    }
    if remaining > Decimal::ZERO || shares <= Decimal::ZERO {
        anyhow::bail!("insufficient Binance Prediction ask depth for ${amount_usdt}");
    }
    Ok(BuyQuote {
        best_bid,
        best_ask,
        spread: best_bid.map(|bid| best_ask - bid),
        best_ask_notional,
        effective_price: amount_usdt / shares,
        limit_price,
        timestamp_ms,
    })
}

fn decimal_to_wei(value: Decimal, label: &str) -> Result<String> {
    if value <= Decimal::ZERO {
        anyhow::bail!("{label} must be positive");
    }
    let wei_scale = Decimal::from_str_exact(WEI_PER_USDT).expect("valid wei scale");
    let wei = value * wei_scale;
    if wei != wei.trunc() {
        anyhow::bail!("{label} exceeds Binance Prediction's 18-decimal precision");
    }
    Ok(wei.normalize().to_string())
}

fn wei_to_decimal(value: &str, label: &str) -> Result<Decimal> {
    let wei = parse_decimal(value, label)?;
    let wei_scale = Decimal::from_str_exact(WEI_PER_USDT).expect("valid wei scale");
    Ok(wei / wei_scale)
}

fn validate_market_quote(
    quote: &binance_sdk::w3w_prediction::rest_api::GetQuoteResponse,
    wallet_address: &str,
    token_id: &str,
    amount_in: &str,
    slippage_bps: u32,
    max_price: Decimal,
) -> Result<()> {
    if quote.token_id.as_deref() != Some(token_id)
        || !quote
            .wallet_address
            .as_deref()
            .is_some_and(|returned| returned.eq_ignore_ascii_case(wallet_address))
        || quote.side.as_deref() != Some("BUY")
        || quote.order_type.as_deref() != Some("MARKET")
        || quote.amount_in.as_deref() != Some(amount_in)
        || quote.slippage_bps != Some(i32::try_from(slippage_bps)?)
    {
        anyhow::bail!("Binance Prediction quote did not match the submitted order");
    }
    if quote
        .expire_at
        .is_some_and(|expiry| expiry <= Utc::now().timestamp_millis())
    {
        anyhow::bail!("Binance Prediction quote expired before placement");
    }
    let min_receive = quote
        .min_receive
        .as_deref()
        .context("Binance Prediction quote omitted minReceive")?;
    let amount = wei_to_decimal(amount_in, "quote amountIn")?;
    let min_shares = wei_to_decimal(min_receive, "quote minReceive")?;
    if min_shares <= Decimal::ZERO || amount / min_shares > max_price {
        anyhow::bail!("Binance Prediction quote exceeds the model value cap");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::test_helpers::d;

    fn active_detail() -> binance_sdk::w3w_prediction::rest_api::GetMarketDetailResponse {
        serde_json::from_str(
            r#"{
                "marketTopicId": 7,
                "vendor": "PREDICT_FUN",
                "slug": "btc-five-minute-7",
                "title": "BTC 5 minute Up or Down",
                "symbol": "BTCUSDT",
                "feeRateBps": 200,
                "startDate": 100000,
                "endDate": 400000,
                "variantData": {
                    "type": "CRYPTO_UP_DOWN",
                    "startPrice": "64000.25",
                    "priceFeedProvider": "BINANCE",
                    "priceFeedSymbol": "BTCUSDT"
                },
                "markets": [{
                    "marketId": 42,
                    "title": "BTC higher than open",
                    "tradingStatus": "OPEN",
                    "outcomes": [
                        {"name": "YES", "tokenId": "up-token"},
                        {"name": "NO", "tokenId": "down-token"}
                    ]
                }]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn parses_active_btc_market_and_yes_no_mapping() {
        let market = parse_active_market(active_detail(), "BTCUSDT", 300_000, 200_000).unwrap();
        assert_eq!(market.market_topic_id, 7);
        assert_eq!(market.reference_price, d("64000.25"));
        assert_eq!(market.up.token_id, "up-token");
        assert_eq!(market.down.token_id, "down-token");
        assert_eq!(market.fee_rate_bps, 200);
    }

    #[test]
    fn rejects_ambiguous_outcome_mapping() {
        let mut value = serde_json::to_value(active_detail()).unwrap();
        value["markets"][0]["title"] = serde_json::json!("BTC up or down");
        let detail = serde_json::from_value(value).unwrap();
        assert!(parse_active_market(detail, "BTCUSDT", 300_000, 200_000).is_err());
    }

    #[test]
    fn walks_multiple_ask_levels_for_fixed_usdt_amount() {
        let quote = build_buy_quote(
            &[(d("0.49"), d("10"))],
            vec![(d("0.52"), d("1")), (d("0.50"), d("1"))],
            d("1"),
            123,
        )
        .unwrap();
        assert_eq!(quote.best_bid, Some(d("0.49")));
        assert_eq!(quote.best_ask, d("0.50"));
        assert_eq!(quote.best_ask_notional, d("0.50"));
        assert_eq!(quote.limit_price, d("0.52"));
        assert!(quote.effective_price > d("0.50"));
        assert!(quote.effective_price < d("0.52"));
    }

    #[test]
    fn rejects_insufficient_order_book_depth() {
        assert!(build_buy_quote(&[], vec![(d("0.50"), d("1"))], d("1"), 123).is_err());
    }

    #[test]
    fn converts_wei_without_floating_point() {
        assert_eq!(
            decimal_to_wei(d("2.125"), "amount").unwrap(),
            "2125000000000000000"
        );
        assert_eq!(
            wei_to_decimal("2125000000000000000", "amount").unwrap(),
            d("2.125")
        );
    }

    #[test]
    fn signs_post_query_without_exposing_secret_or_changing_parameters() {
        let query = signed_query(
            &[
                ("tokenIds".into(), "one".into()),
                ("tokenIds".into(), "two".into()),
            ],
            1_700_000_000_000,
            "secret",
        )
        .unwrap();
        assert!(query.starts_with("tokenIds=one&tokenIds=two&timestamp=1700000000000"));
        assert!(query.contains("&signature="));
        assert!(!query.contains("secret"));
    }

    #[test]
    fn filters_to_current_five_minute_btc_candidates() {
        assert!(topic_matches_candidate(
            Some("BTCUSDT"),
            Some("BTC Price 5 minute Up or Down"),
            None,
            Some(100_000),
            Some(400_000),
            "BTCUSDT",
            300_000,
            200_000,
        ));
        assert!(!topic_matches_candidate(
            Some("ETHUSDT"),
            Some("ETH Price 5 minute Up or Down"),
            None,
            Some(100_000),
            Some(400_000),
            "BTCUSDT",
            300_000,
            200_000,
        ));
    }
}
