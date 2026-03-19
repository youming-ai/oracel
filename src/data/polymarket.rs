//! Polymarket CLOB client — price fetching, order placement, and on-chain redemption via SDK.

use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use futures_util::stream::{self, StreamExt};
use polymarket_client_sdk::auth::state::Authenticated;
use polymarket_client_sdk::auth::{LocalSigner, Normal, Signer as _};
use polymarket_client_sdk::clob;
use polymarket_client_sdk::clob::types::{request::PriceRequest, OrderType, Side};
use polymarket_client_sdk::ctf;
use polymarket_client_sdk::ctf::types::RedeemPositionsRequest;
use polymarket_client_sdk::types::{address, Decimal, U256};
use polymarket_client_sdk::POLYGON;
use std::time::Duration;

use alloy::primitives::{Address, B256};
use alloy::providers::ProviderBuilder;
use alloy::signers::local::PrivateKeySigner;
use secrecy::{ExposeSecret, SecretString};

const CLOB_HOST: &str = "https://clob.polymarket.com";

/// CTF (Conditional Tokens) contract on Polygon mainnet
const CTF_CONTRACT: Address = address!("0x4D97DCd97eC945f40cF65F87097ACe5EA0476045");

alloy::sol! {
    #[sol(rpc)]
    interface ICtfQuery {
        function getCollectionId(bytes32 parentCollectionId, bytes32 conditionId, uint256 indexSet) external view returns (bytes32);
        function getPositionId(address collateralToken, bytes32 collectionId) external pure returns (uint256);
        function balanceOf(address account, uint256 id) external view returns (uint256);
        function payoutDenominator(bytes32 conditionId) external view returns (uint256);
    }
}

/// Unauthenticated client for price queries.
pub(crate) struct PolymarketClient {
    unauth: clob::Client,
}

impl PolymarketClient {
    pub(crate) fn new() -> Result<Self> {
        let config = clob::Config::builder().use_server_time(true).build();
        Ok(Self {
            unauth: clob::Client::new(CLOB_HOST, config)
                .context("Failed to create unauthenticated CLOB client")?,
        })
    }

    pub(crate) async fn fetch_mid_price(&self, token_id: &str) -> Result<f64> {
        let tid = U256::from_str(token_id).context("Invalid token_id")?;
        let req = PriceRequest::builder()
            .token_id(tid)
            .side(Side::Buy)
            .build();
        let result = tokio::time::timeout(Duration::from_secs(10), self.unauth.price(&req))
            .await
            .map_err(|_| anyhow::anyhow!("CLOB price request timed out after 10s"))?
            .context("CLOB price request failed")?;
        let price: f64 = result
            .price
            .try_into()
            .context("Failed to convert Decimal price to f64")?;
        Ok(price)
    }
}

/// Authenticated client for order placement.
pub(crate) struct AuthenticatedPolyClient {
    client: clob::Client<Authenticated<Normal>>,
    signer: PrivateKeySigner,
}

impl AuthenticatedPolyClient {
    pub(crate) async fn new(private_key: &str) -> Result<Self> {
        let key_hex = private_key.strip_prefix("0x").unwrap_or(private_key);
        let signer: PrivateKeySigner = LocalSigner::from_str(key_hex)
            .context("Invalid private key")?
            .with_chain_id(Some(POLYGON));

        let config = clob::Config::builder().use_server_time(true).build();
        let client = clob::Client::new(CLOB_HOST, config)
            .context("Failed to create CLOB client")?
            .authentication_builder(&signer)
            .authenticate();
        let client = tokio::time::timeout(Duration::from_secs(15), client)
            .await
            .map_err(|_| anyhow::anyhow!("CLOB authentication timed out after 15s"))?
            .context("Failed to authenticate with Polymarket CLOB")?;

        Ok(Self { client, signer })
    }

    pub(crate) async fn place_order(
        &self,
        token_id: &str,
        side: &str,
        price: f64,
        size: f64,
    ) -> Result<String> {
        let tid = U256::from_str(token_id).context("Invalid token_id")?;
        let sdk_side = if side == "BUY" { Side::Buy } else { Side::Sell };
        let price_dec = Decimal::try_from(price).context("Invalid price")?;
        let size_dec = Decimal::try_from(size).context("Invalid size")?;

        let order = self
            .client
            .limit_order()
            .token_id(tid)
            .side(sdk_side)
            .price(price_dec)
            .size(size_dec)
            .order_type(OrderType::FOK)
            .build()
            .await
            .context("Failed to build order")?;

        let signed = self
            .client
            .sign(&self.signer, order)
            .await
            .context("Failed to sign order")?;

        let result = tokio::time::timeout(Duration::from_secs(15), self.client.post_order(signed))
            .await
            .map_err(|_| anyhow::anyhow!("Failed to post order: timed out after 15s"))?
            .context("Failed to post order")?;

        Ok(result.order_id)
    }
}

/// USDC on Polygon mainnet
const POLYGON_USDC: alloy::primitives::Address =
    address!("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174");

/// On-chain CTF redeemer for winning outcome tokens.
/// Creates ephemeral provider per redeem (wins are infrequent).
pub(crate) struct CtfRedeemer {
    private_key: SecretString,
    rpc_url: String,
}

impl CtfRedeemer {
    pub(crate) fn new(private_key: String, rpc_url: String) -> Self {
        Self {
            private_key: SecretString::new(private_key.into()),
            rpc_url,
        }
    }

    pub(crate) fn wallet_address(&self) -> Result<Address> {
        let key_hex = self
            .private_key
            .expose_secret()
            .strip_prefix("0x")
            .unwrap_or(self.private_key.expose_secret());
        let signer: PrivateKeySigner =
            LocalSigner::from_str(key_hex).context("Invalid private key")?;
        Ok(signer.address())
    }

    pub(crate) async fn has_redeemable_position(&self, condition_id_hex: &str) -> Result<bool> {
        let wallet_addr = self.wallet_address()?;
        let provider = tokio::time::timeout(
            Duration::from_secs(30),
            ProviderBuilder::new().connect(&self.rpc_url),
        )
        .await
        .map_err(|_| anyhow::anyhow!("RPC connect timed out"))?
        .context("RPC connect failed")?;

        Self::check_single(&provider, wallet_addr, condition_id_hex).await
    }

    pub(crate) async fn find_redeemable(
        &self,
        condition_ids: &[(String, String)],
        concurrency: usize,
    ) -> Result<Vec<(String, String)>> {
        let wallet_addr = self.wallet_address()?;
        let provider = tokio::time::timeout(
            Duration::from_secs(30),
            ProviderBuilder::new().connect(&self.rpc_url),
        )
        .await
        .map_err(|_| anyhow::anyhow!("RPC connect timed out"))?
        .context("RPC connect failed")?;

        let total = condition_ids.len() as u32;
        let checked = Arc::new(AtomicU32::new(0));

        let results: Vec<Option<(String, String)>> = stream::iter(condition_ids.iter().cloned())
            .map(|(cid, slug)| {
                let provider = provider.clone();
                let checked = Arc::clone(&checked);
                async move {
                    let result = Self::check_single(&provider, wallet_addr, &cid).await;
                    let n = checked.fetch_add(1, Ordering::Relaxed) + 1;
                    if n % 50 == 0 || n == total {
                        eprint!("\r  Checked {}/{}", n, total);
                    }
                    match result {
                        Ok(true) => Some((cid, slug)),
                        _ => None,
                    }
                }
            })
            .buffer_unordered(concurrency)
            .collect()
            .await;

        eprintln!();
        Ok(results.into_iter().flatten().collect())
    }

    async fn check_single<P: alloy::providers::Provider>(
        provider: &P,
        wallet_addr: Address,
        condition_id_hex: &str,
    ) -> Result<bool> {
        let hex = condition_id_hex
            .strip_prefix("0x")
            .unwrap_or(condition_id_hex);
        let cid =
            B256::from_str(hex).map_err(|e| anyhow::anyhow!("Invalid condition_id: {}", e))?;

        let ctf = ICtfQuery::new(CTF_CONTRACT, provider);

        let payout = ctf
            .payoutDenominator(cid)
            .call()
            .await
            .map_err(|e| anyhow::anyhow!("payoutDenominator failed: {}", e))?;
        if payout.is_zero() {
            return Ok(false);
        }

        for index_set in [U256::from(1), U256::from(2)] {
            let col = ctf
                .getCollectionId(B256::ZERO, cid, index_set)
                .call()
                .await
                .map_err(|e| anyhow::anyhow!("getCollectionId failed: {}", e))?;
            let pos = ctf
                .getPositionId(POLYGON_USDC, col)
                .call()
                .await
                .map_err(|e| anyhow::anyhow!("getPositionId failed: {}", e))?;
            let bal = ctf
                .balanceOf(wallet_addr, pos)
                .call()
                .await
                .map_err(|e| anyhow::anyhow!("balanceOf failed: {}", e))?;
            if !bal.is_zero() {
                return Ok(true);
            }
        }

        Ok(false)
    }

    pub(crate) async fn redeem(&self, condition_id_hex: &str) -> Result<String> {
        let key_hex = self
            .private_key
            .expose_secret()
            .strip_prefix("0x")
            .unwrap_or(self.private_key.expose_secret());
        let signer: PrivateKeySigner = LocalSigner::from_str(key_hex)
            .context("Invalid private key for CTF")?
            .with_chain_id(Some(POLYGON));

        let wallet = alloy::network::EthereumWallet::from(signer);
        let provider = tokio::time::timeout(
            Duration::from_secs(30),
            ProviderBuilder::new().wallet(wallet).connect(&self.rpc_url),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!("Failed to connect to Polygon RPC for redeem: timed out after 30s")
        })?
        .context("Failed to connect to Polygon RPC for redeem")?;

        let client = ctf::Client::new(provider, POLYGON)
            .map_err(|e| anyhow::anyhow!("CTF client init failed: {}", e))?;

        let hex = condition_id_hex
            .strip_prefix("0x")
            .unwrap_or(condition_id_hex);
        let cid =
            B256::from_str(hex).map_err(|e| anyhow::anyhow!("Invalid condition_id: {}", e))?;

        let req = RedeemPositionsRequest::for_binary_market(POLYGON_USDC, cid);
        let resp = tokio::time::timeout(Duration::from_secs(30), client.redeem_positions(&req))
            .await
            .map_err(|_| anyhow::anyhow!("Redeem failed: timed out after 30s"))?
            .map_err(|e| anyhow::anyhow!("Redeem failed: {}", e))?;

        Ok(format!("{:#x}", resp.transaction_hash))
    }
}
