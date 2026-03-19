# Rust Dependency Analysis

> Generated via librarian agent research (2026-03-19)

## Summary

| Crate | Version | Status | Action |
|-------|---------|--------|--------|
| **alloy** | 1.6 | OK | Consider 1.7.x for fixes |
| **rust_decimal** | 1.x | OK | Watch scale-inconsistency issue #695 |
| **tokio-tungstenite** | 0.24 | OUTDATED | **Upgrade to 0.28** |
| **reqwest** | 0.12 | OK | No action |
| **serde/serde_json** | 1.x | OK | No action |

## 1. alloy v1.6 -- Ethereum Interaction

**Purpose**: Next-generation Rust toolkit for Ethereum/EVM interaction. Complete rewrite of `ethers-rs`, which is **deprecated and archived** (Nov 2023).

**Key API Patterns:**

```rust
// Provider setup
use alloy::providers::{Provider, ProviderBuilder};
let provider = ProviderBuilder::new().connect(rpc_url).await?;

// Contract interaction via sol! macro
sol! {
    #[sol(rpc)]
    contract ERC20 {
        function balanceOf(address owner) public view returns (uint256);
    }
}
let erc20 = ERC20::new(contract_address, provider);
let balance = erc20.balanceOf(owner).call().await?;

// Signing
use alloy::signers::local::PrivateKeySigner;
let signer: PrivateKeySigner = private_key.parse()?;
```

**Project usage**: `alloy = { version = "1.6", features = ["signer-local", "signers", "contract", "providers", "provider-http", "transports", "transport-http"] }` -- correct subset for CLOB signing + RPC calls.

**Known Issues:**
- **RUSTSEC-2026-0002**: alloy <=1.3.0 depends on vulnerable `lru 0.13.0`. v1.6 is safe.
- **serde compatibility**: Earlier alloy versions (1.0.31) had build failures with `serde >=1.0.226`. Fixed in later versions; v1.6 is safe.
- **MSRV**: alloy 1.7+ requires Rust 1.91. v1.6 has a lower MSRV.

**Recommendation**: Consider upgrading to latest 1.7.x for bug fixes. **Do NOT use ethers-rs** -- it's archived.

## 2. rust_decimal v1 -- Precise Decimal Arithmetic

**Purpose**: Decimal number implementation for financial/fixed-precision calculations. 28-digit precision using 96-bit internal representation.

**Key API Patterns:**

```rust
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

// Construction -- prefer from_str, avoid f64 conversion
let price = Decimal::from_str("19.99").unwrap();
let price = dec!(19.99); // macro shorthand

// Arithmetic (all exact)
let total = price * quantity;
```

**Project usage**: `rust_decimal = { version = "1", features = ["serde"] }` -- correct for financial calculations.

**Known Issues:**
- **Scale inconsistency** ([#695](https://github.com/paupino/rust-decimal/issues/695)): Scale depends on computation order when result is zero. Open issue.
- **Max 28 digits**: Hard limit. For Polymarket prices (0.00--1.00), more than sufficient.
- **Division precision**: Order of operations affects rounding in chained divisions.

**Recommendation**: Good choice. Consider adding `maths` feature if you need `pow`, `sqrt`.

## 3. tokio-tungstenite v0.24 -- WebSocket Client (OUTDATED)

**Purpose**: Async WebSocket implementation for Tokio. Wraps `tungstenite-rs` with Tokio bindings.

**Key API Patterns:**

```rust
use tokio_tungstenite::connect_async;
use futures_util::{SinkExt, StreamExt};

let (ws_stream, _) = connect_async("wss://ws.example.com").await?;
let (mut write, mut read) = ws_stream.split();

write.send(Message::Text("subscribe".into())).await?;

while let Some(Ok(msg)) = read.next().await {
    match msg {
        Message::Text(text) => { /* handle */ }
        Message::Close(_) => break,
        _ => {}
    }
}
```

**Project usage**: `tokio-tungstenite = { version = "0.24", features = ["rustls-tls-webpki-roots"] }`

**Known Issues:**
- **Version lag**: Latest is **0.28**, you're on **0.24** (Sep 2024). Four major versions behind.
- **30s disconnect delay** ([#364](https://github.com/snapview/tokio-tungstenite/issues/364)): WebSocket disconnection takes 30 seconds. Root cause is `tungstenite-rs` TCP linger behavior.
- **No built-in reconnection**: Must implement yourself (exponential backoff in a loop).
- **CPU spin on abrupt disconnect** ([#230](https://github.com/snapview/tokio-tungstenite/issues/230)): Fixed in newer versions.

**Recommendation**: **Upgrade to 0.28** for bug fixes and TLS improvements. API is stable across versions.

## 4. reqwest v0.12 -- HTTP Client

**Purpose**: Ergonomic async HTTP client built on `hyper`. De facto standard for Rust HTTP requests.

**Key API Patterns:**

```rust
use reqwest::Client;

// Reuse client for connection pooling
let client = Client::builder()
    .timeout(Duration::from_secs(10))
    .build()?;

let markets: Vec<Market> = client
    .get("https://clob.polymarket.com/markets")
    .send().await?
    .json().await?;
```

**Project usage**: `reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }` -- excellent: using `rustls-tls` instead of `native-tls` avoids OpenSSL dependency, and `default-features = false` keeps the binary lean.

**Notes:**
- `Client` already uses `Arc` internally -- no need to wrap in `Arc<Client>`.
- Always set explicit timeouts; default is no timeout which can hang forever.

**Recommendation**: Perfect as-is. Feature selection is optimal.

## 5. serde / serde_json v1 -- Serialization

**Purpose**: Framework for serializing/deserializing Rust data structures. Format-agnostic.

**Project usage**: `serde = { version = "1", features = ["derive"] }` and `serde_json = "1"` -- standard setup.

**Known Issues:**
- **alloy serde conflict** (historical): Fixed in alloy 1.0.32+. v1.6 is safe.
- **Performance**: For hot-path serialization, consider `simd-json` or `sonic-rs`. For this trading bot, standard `serde_json` is fine.

**Recommendation**: Standard and correct. No changes needed.

## Recommended Alternatives (if needed)

| Concern | Current | Alternative |
|---------|---------|-------------|
| Higher decimal precision | rust_decimal (28 digits) | `bigdecimal` (unlimited) |
| WebSocket with reconnection | tokio-tungstenite (DIY) | `stream-tungstenite` (built-in reconnection) |
| Faster JSON | serde_json | `simd-json` or `sonic-rs` (2-5x faster) |
| Ethereum (if migrating from ethers) | ethers-rs | **alloy** (only option, ethers is dead) |
