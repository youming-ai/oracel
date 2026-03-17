//! Polymarket CLOB client — price fetching and order placement via SDK.

use std::str::FromStr;

use anyhow::{Context, Result};
use polymarket_client_sdk::auth::state::Authenticated;
use polymarket_client_sdk::auth::{LocalSigner, Normal, Signer as _};
use polymarket_client_sdk::clob;
use polymarket_client_sdk::clob::types::{OrderType, Side, request::PriceRequest};
use polymarket_client_sdk::types::{Decimal, U256};
use polymarket_client_sdk::POLYGON;

use alloy::signers::local::PrivateKeySigner;

/// Unauthenticated client for price queries.
pub struct PolymarketClient {
    unauth: clob::Client,
}

impl PolymarketClient {
    pub fn new() -> Self {
        Self {
            unauth: clob::Client::default(),
        }
    }

    pub async fn fetch_mid_price(&self, token_id: &str) -> Result<f64> {
        let tid = U256::from_str(token_id).context("Invalid token_id")?;
        let req = PriceRequest::builder()
            .token_id(tid)
            .side(Side::Buy)
            .build();
        let result = self.unauth.price(&req).await.context("CLOB price request failed")?;
        let price: f64 = result.price.try_into().unwrap_or(0.0);
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

        let client = clob::Client::default()
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
        let tid = U256::from_str(token_id).context("Invalid token_id")?;
        let sdk_side = if side == "BUY" { Side::Buy } else { Side::Sell };
        let price_dec = Decimal::try_from(price).context("Invalid price")?;
        let size_dec = Decimal::try_from(size).context("Invalid size")?;

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
            .context("Failed to post order")?;

        Ok(result.order_id)
    }
}
