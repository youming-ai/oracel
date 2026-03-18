//! Polymarket CLOB client — price fetching, order placement, and on-chain redemption via SDK.

use std::str::FromStr;

use anyhow::{Context, Result};
use polymarket_client_sdk::auth::state::Authenticated;
use polymarket_client_sdk::auth::{LocalSigner, Normal, Signer as _};
use polymarket_client_sdk::clob;
use polymarket_client_sdk::clob::types::{OrderType, Side, request::PriceRequest};
use polymarket_client_sdk::ctf;
use polymarket_client_sdk::ctf::types::RedeemPositionsRequest;
use polymarket_client_sdk::types::{Decimal, U256, address};
use polymarket_client_sdk::POLYGON;

use alloy::primitives::B256;
use alloy::providers::ProviderBuilder;
use alloy::signers::local::PrivateKeySigner;

const CLOB_HOST: &str = "https://clob.polymarket.com";

/// Unauthenticated client for price queries.
pub struct PolymarketClient {
    unauth: clob::Client,
}

impl PolymarketClient {
    pub fn new() -> Self {
        let config = clob::Config::builder().use_server_time(true).build();
        Self {
            unauth: clob::Client::new(CLOB_HOST, config)
                .expect("CLOB client initialization should not fail"),
        }
    }

    pub async fn fetch_mid_price(&self, token_id: &str) -> Result<f64> {
        let tid = U256::from_str(token_id).context("Invalid token_id")?;
        let req = PriceRequest::builder()
            .token_id(tid)
            .side(Side::Buy)
            .build();
        let result = self.unauth.price(&req).await.context("CLOB price request failed")?;
        let price: f64 = result.price.try_into().context("Failed to convert Decimal price to f64")?;
        Ok(price)
    }
}

/// Authenticated client for order placement.
pub struct AuthenticatedPolyClient {
    client: clob::Client<Authenticated<Normal>>,
    signer: PrivateKeySigner,
}

impl AuthenticatedPolyClient {
    pub async fn new(private_key: &str) -> Result<Self> {
        let key_hex = private_key.strip_prefix("0x").unwrap_or(private_key);
        let signer: PrivateKeySigner = LocalSigner::from_str(key_hex)
            .context("Invalid private key")?
            .with_chain_id(Some(POLYGON));

        let config = clob::Config::builder().use_server_time(true).build();
        let client = clob::Client::new(CLOB_HOST, config)
            .context("Failed to create CLOB client")?
            .authentication_builder(&signer)
            .authenticate()
            .await
            .context("Failed to authenticate with Polymarket CLOB")?;

        Ok(Self { client, signer })
    }

    pub async fn place_order(
        &self,
        token_id: &str,
        side: &str,
        price: f64,
        size: f64,
    ) -> Result<String> {
        tracing::info!(
            "[CLOB] placing order: token={} side={} price={:.4} size={:.2}",
            &token_id[..16.min(token_id.len())], side, price, size
        );
        
        let tid = U256::from_str(token_id).context("Invalid token_id")?;
        let sdk_side = if side == "BUY" { Side::Buy } else { Side::Sell };
        
        // Round to avoid floating-point precision issues in Decimal conversion
        // Price: 4 decimals, Size: 2 decimals (per Polymarket API requirements)
        let price_rounded = (price * 10000.0).round() / 10000.0;
        let size_rounded = (size * 100.0).round() / 100.0;
        
        let price_dec = Decimal::try_from(price_rounded).context("Invalid price")?;
        let size_dec = Decimal::try_from(size_rounded).context("Invalid size")?;

        let order = self.client
            .limit_order()
            .token_id(tid)
            .side(sdk_side)
            .price(price_dec)
            .size(size_dec)
            .order_type(OrderType::GTC)
            .build()
            .await
            .context("Failed to build order")?;

        let signed = self.client.sign(&self.signer, order)
            .await
            .context("Failed to sign order")?;

        let result = self.client.post_order(signed)
            .await
            .map_err(|e| {
                tracing::error!(
                    "[CLOB] post_order failed: token={} price={:.4} size={:.2} error={:?}",
                    token_id, price, size, e
                );
                anyhow::anyhow!("Failed to post order: {:?}", e)
            })?;

        tracing::info!("[CLOB] order placed: id={}", result.order_id);
        Ok(result.order_id)
    }

    pub async fn cancel_all(&self) -> Result<usize> {
        let resp = self.client.cancel_all_orders()
            .await
            .context("Failed to cancel all orders")?;
        let count = resp.canceled.len();
        if count > 0 {
            tracing::info!("[CLOB] cancelled {} orders", count);
        }
        Ok(count)
    }
}

/// USDC on Polygon mainnet
const POLYGON_USDC: alloy::primitives::Address = address!("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174");

/// On-chain CTF redeemer for winning outcome tokens.
/// Creates ephemeral provider per redeem (wins are infrequent).
pub struct CtfRedeemer {
    private_key: String,
    rpc_url: String,
}

impl CtfRedeemer {
    pub fn new(private_key: String, rpc_url: String) -> Self {
        Self { private_key, rpc_url }
    }

    /// Redeem winning tokens for a binary market condition back to USDC.
    pub async fn redeem(&self, condition_id_hex: &str) -> Result<String> {
        let key_hex = self.private_key.strip_prefix("0x").unwrap_or(&self.private_key);
        let signer: PrivateKeySigner = LocalSigner::from_str(key_hex)
            .context("Invalid private key for CTF")?
            .with_chain_id(Some(POLYGON));

        let wallet = alloy::network::EthereumWallet::from(signer);
        let provider = ProviderBuilder::new()
            .wallet(wallet)
            .connect(&self.rpc_url)
            .await
            .context("Failed to connect to Polygon RPC for redeem")?;

        let client = ctf::Client::new(provider, POLYGON)
            .map_err(|e| anyhow::anyhow!("CTF client init failed: {}", e))?;

        let hex = condition_id_hex.strip_prefix("0x").unwrap_or(condition_id_hex);
        let cid = B256::from_str(hex)
            .map_err(|e| anyhow::anyhow!("Invalid condition_id: {}", e))?;

        let req = RedeemPositionsRequest::for_binary_market(POLYGON_USDC, cid);
        let resp = client.redeem_positions(&req).await
            .map_err(|e| anyhow::anyhow!("Redeem failed: {}", e))?;

        Ok(format!("{:#x}", resp.transaction_hash))
    }
}
