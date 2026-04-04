# Module Documentation

## Table of Contents
1. [Configuration Module](#configuration-module)
2. [Data Layer](#data-layer)
3. [Pipeline Layer](#pipeline-layer)
4. [CLI Tools](#cli-tools)

---

## Configuration Module

### `src/config.rs`

Central configuration management with validation. Loaded from `config.toml`. All default values are consolidated in a `defaults` submodule for a single source of truth.

#### Key Structures

**`Config`** - Root configuration container
```rust
pub struct Config {
    pub trading: TradingConfig,
    pub market: MarketConfig,
    pub polyclob: PolymarketConfig,
    pub strategy: StrategyConfig,
    pub risk: RiskConfig,
    pub polling: PollingConfig,
    pub price_source: PriceSourceConfig,
    pub execution: ExecutionConfig,
    pub timeouts: TimeoutsConfig,
    pub redeem: RedeemConfig,
    pub misc: MiscConfig,
    pub time_windows: TimeWindowsConfig,
}
```

**`PriceSourceConfig`** - Exchange configuration
```rust
pub struct PriceSourceConfig {
    pub source: PriceSourceType,    // Binance
    pub symbol: String,             // Trading pair (BTCUSDT)
    pub buffer_max: usize,          // Max buffer size
    pub buffer_min_ticks: usize,    // Min ticks before trading
}
```

#### Configuration Validation

The `validate()` method performs startup checks:

| Check | Condition | Error Message |
|-------|-----------|---------------|
| Signal interval | > 0 | `polling.signal_interval_ms must be > 0` |
| Extreme threshold | 0 < value < 1 | `strategy.extreme_threshold must be in (0, 1)` |
| Fair value | 0 < value < 1 | `strategy.fair_value must be in (0, 1)` |
| Position size | > 0 | `strategy.position_size_usdc must be > 0` |
| Symbol format | Binance format | `price_source.symbol must match...` |

#### Symbol Format Validation

**Binance Format** (`BTCUSDT`, `ETHUSDT`):
- Uppercase letters and numbers only
- No dashes or special characters

#### Usage Example

```rust
// Load from file (auto-generated with defaults if missing)
let config = Config::load(Path::new("config.toml"))?;

// Validate before use
config.validate()?;

// Access settings
let symbol = &config.price_source.symbol;
```

---

## Data Layer

### `src/data/binance.rs`

Binance WebSocket client for real-time price feeds.

#### Key Structures

**`BinanceClient`** - WebSocket client with reconnection
```rust
pub struct BinanceClient {
    symbol: String,
    price_tx: broadcast::Sender<TickerUpdate>,
}
```

**`TickerUpdate`** - Price tick with exchange timestamp
```rust
pub struct TickerUpdate {
    pub price: Decimal,
    pub timestamp_ms: i64,  // Binance event time ("E" field)
}
```

**`WsLoopError`** - Error classification for reconnection strategy
```rust
enum WsLoopError {
    Permanent(String),      // Don't retry (invalid symbol)
    Transient(anyhow::Error), // Retry with backoff (network error)
}
```

#### WebSocket Message Format

Binance ticker stream message:
```json
{
  "e": "24hrTicker",    // Event type
  "E": 1234567890000,   // Event time (ms)
  "s": "BTCUSDT",       // Symbol
  "c": "50000.00"       // Current price ("c" = close)
}
```

#### Error Handling

| Error Code | Meaning | Action |
|------------|---------|--------|
| `-1121` | Invalid symbol | Permanent failure, stop reconnecting |
| Network timeout | Connection failed | Retry with exponential backoff |
| Parse error | Invalid message | Log and continue |

#### Reconnection Strategy

```
Initial backoff: 1 second
Max backoff: 60 seconds
Backoff multiplier: 2x
```

#### Usage Example

```rust
let client = Arc::new(BinanceClient::new("BTCUSDT"));
let mut rx = client.subscribe();

// Spawn WebSocket task
tokio::spawn(async move {
    if let Err(e) = client.start_ticker_ws().await {
        tracing::error!("Binance WS stopped: {}", e);
    }
});

// Receive price updates
while let Ok(ticker) = rx.recv().await {
    println!("Price: {} at {}", ticker.price, ticker.timestamp_ms);
}
```

---

### `src/data/market_discovery.rs`

Gamma API integration for market discovery and resolution.

#### Key Structures

**`MarketDiscovery`** - Market discovery client
```rust
pub struct MarketDiscovery {
    gamma_api_url: String,
}
```

**`DiscoveryConfig`** - Configuration for discovery
```rust
pub struct DiscoveryConfig {
    pub gamma_api_url: String,
}
```

**`ResolutionState`** - Market resolution information
```rust
pub struct ResolutionState {
    pub resolved: bool,
    pub outcome: Option<Direction>,  // Up or Down
}
```

#### Key Methods

**`discover()`** - Find current 5-minute market
```rust
pub async fn discover(&self) -> Result<ActiveMarket>
```

**`fetch_market_by_slug()`** - Fetch a specific market by slug
```rust
pub async fn fetch_market_by_slug(&self, slug: &str) -> Result<GammaMarket>
```

**`generate_slug()`** - Generate market slug from timestamp
```rust
pub fn generate_slug(settlement_ms: i64) -> String
// Example: btc-updown-5m-1704067200
```

#### Resolution Detection Logic

```rust
// Market considered resolved when:
1. umaResolutionStatus contains "resolved"
2. closed == true
3. outcomePrices shows one outcome at 1.0, other at 0.0

// Winner determination:
if yes_price == 1.0 → UP wins
if no_price == 1.0 → DOWN wins
```

#### Usage Example

```rust
let discovery = MarketDiscovery::new(DiscoveryConfig {
    gamma_api_url: "https://gamma-api.polymarket.com".to_string(),
});

// Find active market
if let Some(market) = discovery.discover().await? {
    println!("Trading: {} settling at {}",
        market.market_slug,
        market.settlement_time
    );
}

// Fetch market by slug
let gamma_market = discovery.fetch_market_by_slug("btc-updown-5m-1704067200").await?;
```

---

### `src/data/polymarket.rs`

Polymarket CLOB (Central Limit Order Book) client and Polygon RPC URL selection.

#### Key Structures

**`PolymarketClient`** - Unauthenticated client
```rust
pub struct PolymarketClient {
    http_client: reqwest::Client,
}
```

**`AuthenticatedPolyClient`** - Authenticated client for trading
```rust
pub struct AuthenticatedPolyClient {
    sdk_client: ClobClient,
}
```

**`CtfRedeemer`** - On-chain redemption handler
```rust
pub struct CtfRedeemer {
    wallet: LocalWallet,
    provider: Provider<Http>,
}
```

#### Key Methods

**Unauthenticated Operations:**
```rust
// Get order book
pub async fn get_orderbook(&self, token_id: &str) -> Result<OrderBook>

// Get mid prices
pub async fn get_mid_prices(&self, token_ids: &[String]) -> Result<HashMap<String, Decimal>>
```

**Authenticated Operations:**
```rust
// Place FAK order
pub async fn place_fak_order(
    &self,
    token_id: &str,
    side: OrderSide,
    size: &str,
    price: &str,
) -> Result<OrderResponse>

// Get balances
pub async fn get_balances(&self) -> Result<Balances>
```

#### Order Types

**Fill-And-Kill (FAK)**:
- Order must be filled immediately at specified price
- If not fillable, order is cancelled
- Used to ensure known execution price

#### Usage Example

```rust
// Unauthenticated - read market data
let client = PolymarketClient::new()?;
let orderbook = client.get_orderbook(&token_yes_id).await?;

// Authenticated - place trades
let auth = AuthenticatedPolyClient::new(&private_key).await?;
let order = auth.place_fak_order(
    &token_id,
    OrderSide::Buy,
    "100",      // size
    "0.15"      // price
).await?;
```

---

## Pipeline Layer

### `src/pipeline/price_source.rs`

Price source abstraction for Binance WebSocket feeds.

#### Key Structures

**`PriceSource`** - Main price source manager
```rust
pub struct PriceSource {
    client: PriceClient,
    buffer: Arc<RwLock<VecDeque<PriceTick>>>,
    max: usize,
    started: AtomicBool,
}
```

**`PriceClient`** - Enum-based client dispatch
```rust
pub enum PriceClient {
    Binance(Arc<BinanceClient>),
}
```

**`PriceTick`** - Normalized price tick
```rust
pub struct PriceTick {
    pub price: Decimal,
    pub timestamp_ms: i64,
}
```

#### Key Methods

```rust
// Create new price source
pub fn new(source_type: PriceSourceType, symbol: &str, max: usize) -> Self

// Start WebSocket connections
pub async fn start(self: Arc<Self>)

// Get latest price
pub async fn latest(&self) -> Option<Decimal>

// Get last tick timestamp
pub async fn last_tick_ms(&self) -> Option<i64>

// Get buffer length
pub async fn buffer_len(&self) -> usize
```

#### Task Architecture

```
PriceSource::start()
├── Spawn Client Task
│   └── client.start_ticker_ws() (reconnect loop)
└── Spawn Consumer Task
    └── Loop:
        ├── rx.recv() → Get tick
        ├── Check timestamp >= last (monotonic)
        └── Push to buffer (pop front if full)
```

#### Out-of-Order Protection

```rust
// Ignore ticks with timestamps earlier than latest
if h.back().map(|last| ticker.timestamp_ms >= last.timestamp_ms).unwrap_or(true) {
    h.push_back(tick);
} else {
    tracing::debug!("Ignoring out-of-order tick");
}
```

---

### `src/pipeline/decider.rs`

Signal detection, trade decision logic, and risk management. The signal computation is integrated directly into the decider — there is no separate signal module.

#### Key Structures

**`AccountState`** - Account tracking
```rust
pub struct AccountState {
    pub balance: Decimal,
    pub initial_balance: Decimal,
    pub consecutive_losses: u32,
    pub consecutive_wins: u32,
    pub total_wins: u32,
    pub total_losses: u32,
    pub daily_pnl: Decimal,
    pub daily_reset_date: String,
}
```

**`DeciderConfig`** - Decision parameters
```rust
pub struct DeciderConfig {
    pub extreme_threshold: Decimal,
    pub fair_value: Decimal,
    pub position_size_usdc: Decimal,
    pub min_entry_price: Decimal,
    pub max_entry_price: Decimal,
    pub min_ttl_for_entry_ms: u64,
    pub daily_loss_limit_usdc: Decimal,
}
```

**`Decision`** - Trade decision result
```rust
pub enum Decision {
    Pass(String),           // Reason for no trade
    Trade {
        direction: Direction,
        size_usdc: Decimal,
        edge: Decimal,
        payoff_ratio: Decimal,  // (1 - cheap_price) / cheap_price
    },
}
```

**`Direction`** - Trade direction
```rust
pub enum Direction {
    Up,    // Market extremely bearish, buy UP
    Down,  // Market extremely bullish, buy DOWN
}
```

#### Decision Pipeline

```
decide()
├── 1. Market data valid? → Pass("no_market_data")
├── 2. Spread check? → Pass("spread_too_wide")
├── 3. Extreme check? → Pass("not_extreme_XX%")
├── 4. Entry price range? → Pass("entry_price_out_of_range")
├── 5. Min TTL for entry? → Pass("ttl_too_short")
├── 6. Balance > 0? → Pass("insufficient_balance")
├── 7. Daily loss limit? → Pass("daily_loss_limit")
└── TRADE: Calculate position size
```

---

### `src/pipeline/executor.rs`

Order execution module for paper and live trading.

#### Key Structures

**`Executor`** - Execution coordinator
```rust
pub struct Executor {
    mode: TradingMode,
    auth_client: Option<AuthenticatedPolyClient>,
}
```

**`ExecuteContext`** - Execution parameters
```rust
pub struct ExecuteContext<'a> {
    pub decision: &'a Decision,
    pub token_yes: &'a str,
    pub token_no: &'a str,
    pub poly_yes: Option<Decimal>,
    pub poly_no: Option<Decimal>,
    pub settlement_time_ms: i64,
    pub btc_price: Decimal,
}
```

#### Execution Modes

**Paper Mode**: Generates UUID order ID, calculates shares, returns `PaperOrder`.

**Live Mode**: Places FAK order via CLOB, returns `LiveOrder`.

Both modes apply zero-share guard (reject if shares == 0).

---

### `src/pipeline/settler.rs`

Position settlement and PnL tracking.

#### Key Structures

**`PendingPosition`** - Position awaiting settlement
```rust
pub struct PendingPosition {
    pub direction: Direction,
    pub size_usdc: Decimal,
    pub entry_price: Decimal,
    pub filled_shares: Decimal,
    pub cost: Decimal,
    pub settlement_time_ms: i64,
    pub condition_id: String,
    pub market_slug: String,
}
```

**`SettlementResult`** - Settlement outcome
```rust
pub struct SettlementResult {
    pub direction: Direction,
    pub payout: Decimal,
    pub pnl: Decimal,
    pub won: bool,
    pub condition_id: String,
}
```

**`Settler`** - Settlement manager
```rust
pub struct Settler {
    pending: VecDeque<PendingPosition>,
}
```

#### Key Methods

```rust
pub fn add_position(&mut self, pos: PendingPosition)  // prevents duplicates
pub fn due_positions(&self) -> Vec<PendingPosition>    // past settlement time
pub fn settle_by_slug(&mut self, slug: &str, won: bool) -> Option<SettlementResult>
```

#### Duplicate Prevention

```rust
fn add_position(&mut self, pos: PendingPosition) {
    if self.pending.iter().any(|p| p.condition_id == pos.condition_id) {
        tracing::warn!("Duplicate position, skipping");
        return;
    }
    self.pending.push_back(pos);
}
```

---

## CLI Tools

### `src/cli.rs`

Separate binary (`polybot-tools`) for utility commands that are not part of the trading loop.

#### Commands

```bash
polybot-tools --derive-keys        # Derive Polymarket CLOB API credentials
polybot-tools --redeem-all         # Redeem all winning positions (last 24h)
polybot-tools --redeem <slug>      # Redeem a single market by slug
```

#### Shared Library

Both `polybot` and `polybot-tools` binaries share code via `src/lib.rs`, which re-exports:
- `config` — configuration types and validation
- `data` — exchange clients, Polymarket CLOB, market discovery
- `pipeline` — trading pipeline stages
