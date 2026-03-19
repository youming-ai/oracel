# Polymarket CLOB API Reference

> Generated via librarian agent research (2026-03-19)

## 1. API Architecture

Polymarket is served by **three separate APIs**:

| API | Base URL | Purpose |
|-----|----------|---------|
| **Gamma API** | `https://gamma-api.polymarket.com` | Markets, events, tags, search, profiles |
| **Data API** | `https://data-api.polymarket.com` | Positions, trades, activity, leaderboards |
| **CLOB API** | `https://clob.polymarket.com` | Orderbook, pricing, order placement/cancellation |
| Bridge API | `https://bridge.polymarket.com` | Deposits/withdrawals (proxy of fun.xyz) |

## 2. CLOB REST Endpoints

### Public (No Auth Required)

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/time` | Server Unix timestamp |
| `GET` | `/midpoint?token_id=` | Midpoint price for one token |
| `GET` | `/midpoints?token_ids=` | Midpoint prices (comma-separated) |
| `POST` | `/midpoints` | Midpoint prices (request body) |
| `GET` | `/price?token_id=&side=` | Best bid/ask price |
| `GET` | `/prices` | Best prices for multiple tokens |
| `POST` | `/prices` | Best prices (request body) |
| `GET` | `/spread?token_id=` | Bid-ask spread |
| `GET` | `/spreads` | Spreads for multiple tokens |
| `GET` | `/book?token_id=` | Full orderbook snapshot |
| `POST` | `/books` | Orderbooks for multiple tokens |
| `GET` | `/tick-size/:token_id` | Minimum tick size |
| `GET` | `/fee-rate/:token_id` | Base fee rate |
| `GET` | `/last-trade-price?token_id=` | Last trade price + side |
| `GET` | `/last-trade-prices` | Last trade prices (batch) |
| `GET` | `/prices-history` | Historical price data |

### Authenticated (L2 Headers Required)

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/order` | Place a single order |
| `POST` | `/orders` | Place up to 15 orders (batch) |
| `DELETE` | `/order` | Cancel single order |
| `DELETE` | `/orders` | Cancel up to 3000 orders |
| `DELETE` | `/cancel-all` | Cancel all open orders |
| `DELETE` | `/cancel-market-orders` | Cancel orders for a market |
| `GET` | `/orders` | Get user's open orders (paginated) |
| `GET` | `/order/:orderID` | Get single order by ID |
| `GET` | `/trades` | Get user's trades (paginated) |
| `POST` | `/heartbeat` | Send session heartbeat |

### L1 Auth (Private Key)

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/auth/api-key` | Create new API credentials |
| `GET` | `/auth/derive-api-key` | Derive existing API credentials |

## 3. Authentication Flow

### Two-Level Model

**L1 (Private Key)** -> Creates/derives API credentials
**L2 (API Key)** -> Authenticates trading requests via HMAC-SHA256

### Step 1: Get API Credentials (L1)

Sign an EIP-712 message with your wallet's private key:

```typescript
// EIP-712 domain
const domain = {
  name: "ClobAuthDomain",
  version: "1",
  chainId: 137, // Polygon
};

const types = {
  ClobAuth: [
    { name: "address", type: "address" },
    { name: "timestamp", type: "string" },
    { name: "nonce", type: "uint256" },
    { name: "message", type: "string" },
  ],
};

const value = {
  address: signingAddress,
  timestamp: serverTimestamp,  // from GET /time
  nonce: 0,
  message: "This message attests that I control the given wallet",
};
```

**L1 Headers** for `POST /auth/api-key` or `GET /auth/derive-api-key`:

| Header | Description |
|--------|-------------|
| `POLY_ADDRESS` | Polygon signer address |
| `POLY_SIGNATURE` | EIP-712 signature |
| `POLY_TIMESTAMP` | Current UNIX timestamp |
| `POLY_NONCE` | Nonce (default: 0) |

**Response:**
```json
{
  "apiKey": "550e8400-e29b-41d4-a716-446655440000",
  "secret": "base64EncodedSecretString",
  "passphrase": "randomPassphraseString"
}
```

### Step 2: Sign Requests (L2 -- HMAC-SHA256)

All trading endpoints require 5 headers:

| Header | Description |
|--------|-------------|
| `POLY_ADDRESS` | Polygon signer address |
| `POLY_SIGNATURE` | HMAC-SHA256 signature |
| `POLY_TIMESTAMP` | Current UNIX timestamp |
| `POLY_API_KEY` | API key from Step 1 |
| `POLY_PASSPHRASE` | Passphrase from Step 1 |

### Signature Types

| Type | Value | Description |
|------|-------|-------------|
| EOA | `0` | Standard wallet (MetaMask). Funder = EOA address |
| POLY_PROXY | `1` | Magic Link email/Google proxy wallet |
| GNOSIS_SAFE | `2` | Gnosis Safe multisig (most common for new users) |

### Rust SDK Auth

```rust
use polymarket_client_sdk::POLYGON;
use polymarket_client_sdk::auth::{LocalSigner, Signer};
use polymarket_client_sdk::clob::{Client, Config};

let signer = LocalSigner::from_str(&private_key)?
    .with_chain_id(Some(POLYGON));

let client = Client::new("https://clob.polymarket.com", Config::default())?
    .authentication_builder(&signer)
    .signature_type(SignatureType::Proxy)
    .authenticate()
    .await?;
```

## 4. Order Types

| Type | Behavior | Use Case |
|------|----------|----------|
| **GTC** | Good-Til-Cancelled -- rests on book until filled or cancelled | Default for limit orders |
| **GTD** | Good-Til-Date -- active until specified expiration | Auto-expire before known events |
| **FOK** | Fill-Or-Kill -- must fill entirely or cancel | All-or-nothing market orders |
| **FAK** | Fill-And-Kill -- fills available, cancels rest | Partial-fill market orders |

**Key details:**
- All orders are fundamentally **limit orders** -- market orders are limit orders with a marketable price
- **GTC/GTD** rest on the book; **FOK/FAK** execute immediately against resting liquidity
- **Post-only** mode (GTC/GTD only): guarantees maker status, rejects if would cross the spread
- **BUY**: specify dollar amount; **SELL**: specify number of shares
- Market orders use `price` as **worst-price limit** (slippage protection)
- GTD has a 60-second security threshold: use `now + 60 + N` for N-second lifetime
- Tick sizes: `0.1`, `0.01`, `0.001`, `0.0001`
- Batch: up to **15 orders** per request
- Cancel batch: up to **3000 orders** per request

**Rust SDK examples:**
```rust
// Limit order (GTC)
let order = client.limit_order()
    .token_id("TOKEN_ID".parse()?)
    .price(dec!(0.50))
    .size(dec!(10))
    .side(Side::Buy)
    .build().await?;

// Market order (FOK)
let order = client.market_order()
    .token_id(token_id)
    .amount(Amount::usdc(dec!(100))?)
    .price(dec!(0.50))  // worst-price limit
    .side(Side::Buy)
    .order_type(OrderType::FOK)
    .build().await?;
```

### Order Response Statuses

| Status | Description |
|--------|-------------|
| `live` | Resting on the book |
| `matched` | Immediately matched |
| `delayed` | Subject to matching delay (sports markets) |
| `unmatched` | Marketable but delay failed |

### Heartbeat

If no heartbeat within **10 seconds**, all open orders are auto-cancelled. Send every 5 seconds:

```rust
Client::start_heartbeats(&mut client)?;
```

## 5. WebSocket Channels

### Channel Overview

| Channel | Endpoint | Auth |
|---------|----------|------|
| **Market** | `wss://ws-subscriptions-clob.polymarket.com/ws/market` | No |
| **User** | `wss://ws-subscriptions-clob.polymarket.com/ws/user` | Yes |

### Market Channel (Public)

**Subscribe:**
```json
{
  "assets_ids": ["<token_id_1>", "<token_id_2>"],
  "type": "market",
  "custom_feature_enabled": true
}
```

**Message Types:**

| `event_type` | Description |
|--------------|-------------|
| `book` | Full orderbook snapshot (bids/asks) |
| `price_change` | Price level updates (new/cancelled orders) |
| `tick_size_change` | Tick size changes |
| `last_trade_price` | Trade executions |
| `best_bid_ask` | Best bid/ask updates (custom feature) |
| `new_market` | New market created (custom feature) |
| `market_resolved` | Market resolution (custom feature) |

**`book` message format:**
```json
{
  "event_type": "book",
  "asset_id": "658186...",
  "market": "0xbd31dc...",
  "bids": [{ "price": ".48", "size": "30" }],
  "asks": [{ "price": ".52", "size": "25" }],
  "timestamp": "123456789000",
  "hash": "0x0...."
}
```

**`price_change` format:**
```json
{
  "event_type": "price_change",
  "market": "0x5f65...",
  "price_changes": [{
    "asset_id": "713210...",
    "price": "0.5",
    "size": "200",
    "side": "BUY",
    "best_bid": "0.5",
    "best_ask": "1"
  }],
  "timestamp": "1757908892351"
}
```

### User Channel (Authenticated)

**Subscribe:**
```json
{
  "auth": {
    "apiKey": "your-api-key",
    "secret": "your-api-secret",
    "passphrase": "your-passphrase"
  },
  "markets": ["0x1234...condition_id"],
  "type": "user"
}
```

Note: subscribes by **condition_id** (not asset_id).

**Trade statuses:** `MATCHED` -> `MINED` -> `CONFIRMED` (or `RETRYING` -> `FAILED`)

### Heartbeats

- **Market/User**: Send `PING` every 10 seconds, server responds `PONG`

### Dynamic Subscription

```json
{ "assets_ids": ["new_id"], "operation": "subscribe" }
{ "assets_ids": ["old_id"], "operation": "unsubscribe" }
```

## 6. Official SDK Repositories

| Language | Package | Repository |
|----------|---------|------------|
| **Rust** | `polymarket-client-sdk` | [github.com/Polymarket/rs-clob-client](https://github.com/Polymarket/rs-clob-client) |
| **TypeScript** | `@polymarket/clob-client` | [github.com/Polymarket/clob-client](https://github.com/Polymarket/clob-client) |
| **Python** | `py-clob-client` | [github.com/Polymarket/py-clob-client](https://github.com/Polymarket/py-clob-client) |

## 7. OpenAPI Specs

| Spec | URL |
|------|-----|
| CLOB API | `https://docs.polymarket.com/api-spec/clob-openapi.yaml` |
| Gamma API | `https://docs.polymarket.com/api-spec/gamma-openapi.yaml` |
| Data API | `https://docs.polymarket.com/api-spec/data-openapi.yaml` |
| WebSocket AsyncAPI | `https://docs.polymarket.com/asyncapi.json` |
