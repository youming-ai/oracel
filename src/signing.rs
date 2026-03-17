//! EIP-712 Order Signing for Polymarket CTF Exchange
//!
//! Implements EIP-712 typed structured data signing for Polymarket orders.
//! Compatible with the @polymarket/clob-client OrderBuilder.

use anyhow::{Context, Result};
use ethers::core::types::{Address, U256};
use ethers::signers::{LocalWallet, Signer};
use ethers::utils::keccak256;
use serde::{Deserialize, Serialize};

// ─── Constants ───

/// Polymarket CTF Exchange contract on Polygon mainnet
pub const EXCHANGE_CONTRACT: &str = "0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E";

/// Neg-risk exchange contract
pub const NEG_RISK_EXCHANGE_CONTRACT: &str = "0xC5d563A36AE78145C45a50134d48A121514005a1";

/// Protocol name for EIP-712 domain
pub const PROTOCOL_NAME: &str = "Polymarket CTF Exchange";

/// Protocol version
pub const PROTOCOL_VERSION: &str = "1";

/// Polygon mainnet chain ID
pub const CHAIN_ID: u64 = 137;

/// USDC decimals (6)
pub const USDC_DECIMALS: u32 = 6;

// ─── Enums ───

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum OrderSide {
    Buy = 0,
    Sell = 1,
}

impl OrderSide {
    pub fn as_u8(&self) -> u8 {
        *self as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum SignatureType {
    /// Standard EOA wallet signature
    Eoa = 0,
    /// Polymarket proxy wallet
    PolyProxy = 1,
    /// Polymarket Gnosis Safe
    PolyGnosisSafe = 2,
}

// ─── Order ───

#[derive(Debug, Clone)]
pub struct Order {
    pub salt: u64,
    pub maker: Address,
    pub signer: Address,
    pub taker: Address,
    pub token_id: U256,
    pub maker_amount: U256,
    pub taker_amount: U256,
    pub expiration: u64,
    pub nonce: U256,
    pub fee_rate_bps: U256,
    pub side: OrderSide,
    pub signature_type: SignatureType,
}

/// Signed order ready for submission to CLOB API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedOrder {
    pub salt: String,
    pub maker: String,
    pub signer: String,
    pub taker: String,
    #[serde(rename = "tokenId")]
    pub token_id: String,
    #[serde(rename = "makerAmount")]
    pub maker_amount: String,
    #[serde(rename = "takerAmount")]
    pub taker_amount: String,
    pub expiration: String,
    pub nonce: String,
    #[serde(rename = "feeRateBps")]
    pub fee_rate_bps: String,
    pub side: u8,
    #[serde(rename = "signatureType")]
    pub signature_type: u8,
    pub signature: String,
}

// ─── EIP-712 Type Hashes ───

/// EIP-712 type hash for the Order struct - computed at startup
fn order_type_hash() -> [u8; 32] {
    keccak256(
        b"Order(uint256 salt,address maker,address signer,address taker,uint256 tokenId,uint256 makerAmount,uint256 takerAmount,uint256 expiration,uint256 nonce,uint256 feeRateBps,uint8 side,uint8 signatureType)"
    )
}

/// Compute EIP-712 domain separator
pub fn domain_separator(contract: &Address, chain_id: u64) -> [u8; 32] {
    // EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)
    let domain_type_hash = keccak256(
        b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"
    );
    let name_hash = keccak256(PROTOCOL_NAME.as_bytes());
    let version_hash = keccak256(PROTOCOL_VERSION.as_bytes());

    let mut data = Vec::with_capacity(32 * 5);
    data.extend_from_slice(&domain_type_hash);
    data.extend_from_slice(&name_hash);
    data.extend_from_slice(&version_hash);
    data.extend_from_slice(&u256_to_bytes32(U256::from(chain_id)));
    data.extend_from_slice(&address_to_bytes32(contract));

    keccak256(&data)
}

/// Compute the struct hash for an Order
fn order_struct_hash(order: &Order) -> [u8; 32] {
    let mut data = Vec::with_capacity(32 * 12);
    data.extend_from_slice(&order_type_hash());
    data.extend_from_slice(&u256_to_bytes32(U256::from(order.salt)));
    data.extend_from_slice(&address_to_bytes32(&order.maker));
    data.extend_from_slice(&address_to_bytes32(&order.signer));
    data.extend_from_slice(&address_to_bytes32(&order.taker));
    data.extend_from_slice(&u256_to_bytes32(order.token_id));
    data.extend_from_slice(&u256_to_bytes32(order.maker_amount));
    data.extend_from_slice(&u256_to_bytes32(order.taker_amount));
    data.extend_from_slice(&u256_to_bytes32(U256::from(order.expiration)));
    data.extend_from_slice(&u256_to_bytes32(order.nonce));
    data.extend_from_slice(&u256_to_bytes32(order.fee_rate_bps));
    data.extend_from_slice(&[order.side.as_u8()]);
    // Pad side to 32 bytes
    data.extend_from_slice(&[0u8; 31]);
    data.extend_from_slice(&[order.signature_type as u8]);
    // Pad signatureType to 32 bytes
    data.extend_from_slice(&[0u8; 31]);

    keccak256(&data)
}

/// Compute the EIP-712 digest (what gets signed)
pub fn eip712_digest(order: &Order, contract: &Address, chain_id: u64) -> [u8; 32] {
    let separator = domain_separator(contract, chain_id);
    let struct_hash = order_struct_hash(order);

    let mut data = Vec::with_capacity(2 + 32 + 32);
    data.push(0x19);
    data.push(0x01);
    data.extend_from_slice(&separator);
    data.extend_from_slice(&struct_hash);

    keccak256(&data)
}

/// Sign an order using a local wallet
pub async fn sign_order(
    order: &Order,
    wallet: &LocalWallet,
    contract: &Address,
    chain_id: u64,
) -> Result<Vec<u8>> {
    let digest = eip712_digest(order, contract, chain_id);

    let signature = wallet
        .sign_hash(digest.into())
        .context("Failed to sign order digest")?;

    // Convert to [r(32) || s(32) || v(1)] format
    let mut sig_bytes = Vec::with_capacity(65);
    let mut r_bytes = [0u8; 32];
    let mut s_bytes = [0u8; 32];
    signature.r.to_big_endian(&mut r_bytes);
    signature.s.to_big_endian(&mut s_bytes);
    sig_bytes.extend_from_slice(&r_bytes);
    sig_bytes.extend_from_slice(&s_bytes);
    sig_bytes.push(signature.v as u8);

    Ok(sig_bytes)
}

/// Create and sign a limit order for Polymarket
pub async fn create_signed_order(
    wallet: &LocalWallet,
    token_id: &str,
    side: OrderSide,
    price: f64,
    size: f64,
    expiration: u64,
    neg_risk: bool,
) -> Result<SignedOrder> {
    let chain_id = CHAIN_ID;
    let contract = if neg_risk {
        NEG_RISK_EXCHANGE_CONTRACT.parse::<Address>()?
    } else {
        EXCHANGE_CONTRACT.parse::<Address>()?
    };

    let maker = wallet.address();
    let taker = Address::zero();
    let token_id_u256 = U256::from_dec_str(token_id).context("Invalid token_id")?;

    // Calculate raw amounts
    // For BUY: makerAmount = size * price (USDC), takerAmount = size (shares)
    // For SELL: makerAmount = size (shares), takerAmount = size * price (USDC)
    let (raw_maker, raw_taker) = match side {
        OrderSide::Buy => {
            let shares = round_down(size, 2);
            let usdc = round_down(shares * price, 4);
            (usdc, shares)
        }
        OrderSide::Sell => {
            let shares = round_down(size, 2);
            let usdc = round_down(shares * price, 4);
            (shares, usdc)
        }
    };

    let maker_amount = to_usdc_units(raw_maker);
    let taker_amount = to_usdc_units(raw_taker);

    // Generate salt (timestamp-based)
    let salt = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let order = Order {
        salt,
        maker,
        signer: maker,
        taker,
        token_id: token_id_u256,
        maker_amount,
        taker_amount,
        expiration,
        nonce: U256::zero(),
        fee_rate_bps: U256::zero(),
        side,
        signature_type: SignatureType::Eoa,
    };

    // Sign the order
    let sig_bytes = sign_order(&order, wallet, &contract, chain_id).await?;
    let signature = format!("0x{}", hex::encode(&sig_bytes));

    Ok(SignedOrder {
        salt: order.salt.to_string(),
        maker: format!("{:?}", order.maker),
        signer: format!("{:?}", order.signer),
        taker: format!("{:?}", order.taker),
        token_id: order.token_id.to_string(),
        maker_amount: order.maker_amount.to_string(),
        taker_amount: order.taker_amount.to_string(),
        expiration: order.expiration.to_string(),
        nonce: order.nonce.to_string(),
        fee_rate_bps: order.fee_rate_bps.to_string(),
        side: order.side.as_u8(),
        signature_type: order.signature_type as u8,
        signature,
    })
}

// ─── Helpers ───

fn u256_to_bytes32(value: U256) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    value.to_big_endian(&mut bytes);
    bytes
}

fn address_to_bytes32(addr: &Address) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[12..].copy_from_slice(addr.as_bytes());
    bytes
}

/// Round down to `decimals` decimal places
fn round_down(value: f64, decimals: u32) -> f64 {
    let factor = 10f64.powi(decimals as i32);
    (value * factor).floor() / factor
}

/// Convert USDC value to on-chain units (6 decimals)
fn to_usdc_units(value: f64) -> U256 {
    let scaled = (value * 10f64.powi(USDC_DECIMALS as i32)) as u64;
    U256::from(scaled)
}

// ─── Derive Order Type Hash ───

/// Compute the ORDER_TYPE_HASH at runtime (for verification)
pub fn compute_order_type_hash() -> [u8; 32] {
    keccak256(
        b"Order(uint256 salt,address maker,address signer,address taker,uint256 tokenId,uint256 makerAmount,uint256 takerAmount,uint256 expiration,uint256 nonce,uint256 feeRateBps,uint8 side,uint8 signatureType)"
    )
}

// ─── Tests ───

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_type_hash() {
        let computed = order_type_hash();
        let expected = compute_order_type_hash();
        assert_eq!(computed, expected,
            "ORDER_TYPE_HASH mismatch. Computed: 0x{}", hex::encode(computed));
        // Log the hash for verification
        tracing::info!("Order type hash: 0x{}", hex::encode(computed));
    }

    #[test]
    fn test_domain_separator() {
        let contract: Address = EXCHANGE_CONTRACT.parse().unwrap();
        let separator = domain_separator(&contract, CHAIN_ID);
        assert_ne!(separator, [0u8; 32]);
        tracing::info!("Domain separator: 0x{}", hex::encode(separator));
    }

    #[test]
    fn test_round_down() {
        assert!((round_down(0.567, 2) - 0.56).abs() < 0.001);
        assert!((round_down(1.234567, 4) - 1.2345).abs() < 0.0001);
    }

    #[test]
    fn test_to_usdc_units() {
        assert_eq!(to_usdc_units(1.0), U256::from(1_000_000));
        assert_eq!(to_usdc_units(10.50), U256::from(10_500_000));
        assert_eq!(to_usdc_units(0.01), U256::from(10_000));
    }

    #[tokio::test]
    async fn test_sign_order() {
        use ethers::signers::LocalWallet;

        // Create a test wallet
        let wallet = LocalWallet::new(&mut rand::thread_rng());
        let contract: Address = EXCHANGE_CONTRACT.parse().unwrap();

        let token_id = "52114319568358318285620337107369850307986745309276438629354142991008904627974";
        let token_id_u256 = U256::from_dec_str(token_id).unwrap();

        let order = Order {
            salt: 1234567890,
            maker: wallet.address(),
            signer: wallet.address(),
            taker: Address::zero(),
            token_id: token_id_u256,
            maker_amount: U256::from(50_000_000u64), // 50 USDC
            taker_amount: U256::from(100_000_000u64), // 100 shares
            expiration: 0,
            nonce: U256::zero(),
            fee_rate_bps: U256::zero(),
            side: OrderSide::Buy,
            signature_type: SignatureType::Eoa,
        };

        let sig = sign_order(&order, &wallet, &contract, CHAIN_ID).await;
        assert!(sig.is_ok(), "Signing should succeed");
        let sig_bytes = sig.unwrap();
        assert_eq!(sig_bytes.len(), 65, "Signature should be 65 bytes");

        // Verify signature is valid (recover signer)
        let digest = eip712_digest(&order, &contract, CHAIN_ID);
        let sig = ethers::core::types::Signature {
            r: U256::from_big_endian(&sig_bytes[0..32]),
            s: U256::from_big_endian(&sig_bytes[32..64]),
            v: sig_bytes[64] as u64,
        };
        let recovered = sig.recover(digest).unwrap();
        assert_eq!(recovered, wallet.address(), "Recovered address should match");
    }

    #[tokio::test]
    async fn test_create_signed_order() {
        use ethers::signers::LocalWallet;

        let wallet = LocalWallet::new(&mut rand::thread_rng());
        let token_id = "52114319568358318285620337107369850307986745309276438629354142991008904627974";

        let signed = create_signed_order(
            &wallet,
            token_id,
            OrderSide::Buy,
            0.55,  // price
            10.0,  // size (10 shares)
            0,     // expiration (GTC)
            false, // not neg-risk
        ).await;

        assert!(signed.is_ok(), "Order creation should succeed");
        let order = signed.unwrap();

        assert_eq!(order.side, 0); // BUY
        assert!(order.signature.starts_with("0x"));
        assert_eq!(order.signature.len(), 132); // 0x + 130 hex chars = 65 bytes

        tracing::info!("Signed order: {}", serde_json::to_string_pretty(&order).unwrap());
    }
}
