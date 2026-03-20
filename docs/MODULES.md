# Module Documentation

## Table of Contents
1. [Configuration Module](#configuration-module)
2. [Data Layer](#data-layer)
3. [Pipeline Layer](#pipeline-layer)

---

## Configuration Module

### `src/config.rs`

Central configuration management with validation and defaults.

#### Key Structures

**`Config`** - Root configuration container
```rust
pub struct Config {
    pub trading: TradingConfig,
    pub market: MarketConfig,
    pub polyclob: PolymarketConfig,
    pub strategy: StrategyConfig,
    pub edge: EdgeConfigFile,
    pub risk: RiskConfig,
    pub polling: PollingConfig,
    pub price_source: PriceSourceConfig,
}
```

**`RiskConfig`** - Risk management settings
```rust
pub struct RiskConfig {
    pub max_consecutive_losses: u32,     // Circuit breaker threshold
    pub max_daily_loss_pct: Decimal,     // Daily loss limit (0.10 = 10%)
    pub cooldown_ms: i64,                // Minimum time between trades
    pub enforce_limits: bool,            // Advisory vs strict mode
}
```

**`PriceSourceConfig`** - Exchange configuration
```rust
pub struct PriceSourceConfig {
    pub source: PriceSourceType,    // Binance, Coinbase, etc.
    pub symbol: String,             // Trading pair (BTCUSDT, BTC-USD)
}
```

#### Configuration Validation

The `validate()` method performs startup checks:

| Check | Condition | Error Message |
|-------|-----------|---------------|
| Signal interval | > 0 | `polling.signal_interval_ms must be > 0` |
| Extreme threshold | 0 < value < 1 | `strategy.extreme_threshold must be in (0, 1)` |
| Fair value | 0 < value < 1 | `strategy.fair_value must be in (0, 1)` |
| Daily loss pct | 0 < value ≤ 1 | `risk.max_daily_loss_pct must be in (0, 1]` |
| Window minutes | > 0 | `market.window_minutes must be > 0` |
| Symbol format | Exchange-specific | `price_source.symbol must match...` |

#### Symbol Format Validation

**Binance Format** (`BTCUSDT`, `ETHUSDT`):
- Uppercase letters and numbers only
- No dashes or special characters
- Minimum 1 character

**Coinbase Format** (`BTC-USD`, `ETH-USD`):
- Exactly one dash separator
- Base and quote currencies must be alphanumeric
- Both sides must be non-empty

#### Usage Example

```rust
// Load from file
let config = Config::load(Path::new("config.json"))?;

// Validate before use
config.validate()?;

// Access settings
let cooldown = config.risk.cooldown_ms;
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
    latest_price: Arc<RwLock<Option<f64>>>,
}
```

**`TickerUpdate`** - Price tick with exchange timestamp
```rust
pub struct TickerUpdate {
    pub price: f64,
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
Jitter: None (deterministic)
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

### `src/data/coinbase.rs`

Coinbase Advanced Trade WebSocket client.

#### Key Structures

**`CoinbaseClient`** - WebSocket client
```rust
pub struct CoinbaseClient {
    product_id: String,
    price_tx: broadcast::Sender<TickerUpdate>,
    latest_price: Arc<RwLock<Option<f64>>>,
}
```

#### WebSocket Message Format

Coinbase ticker message:
```json
{
  "channel": "ticker",
  "events": [{
    "tickers": [{
      "product_id": "BTC-USD",
      "price": "50000.00"
    }]
  }]
}
```

#### Differences from Binance

| Aspect | Binance | Coinbase |
|--------|---------|----------|
| Symbol format | BTCUSDT | BTC-USD |
| Timestamp field | "E" (event time) | Not provided (uses local time) |
| Stream name | `{symbol}@ticker` | `ticker` channel |
| Price field | "c" (close) | "price" |

#### Usage Example

```rust
let client = Arc::new(CoinbaseClient::new("BTC-USD"));
let mut rx = client.subscribe();

// Usage identical to Binance
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

**`find_active_market()`** - Find current 5-minute market
```rust
pub async fn find_active_market(&self) -> Result<Option<ActiveMarket>>
```

**`check_resolution()`** - Check if market is resolved
```rust
pub async fn check_resolution(&self, condition_id: &str) -> Result<ResolutionState>
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
if let Some(market) = discovery.find_active_market().await? {
    println!("Trading: {} settling at {}", 
        market.market_slug, 
        market.settlement_time
    );
}

// Check resolution
let state = discovery.check_resolution(&condition_id).await?;
if state.resolved {
    println!("Winner: {:?}", state.outcome);
}
```

---

### `src/data/polymarket.rs`

Polymarket CLOB (Central Limit Order Book) client.

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
pub async fn get_mid_prices(&self, token_ids: &[String]) -> Result<HashMap<String, f64>>
```

**Authenticated Operations:**
```rust
// Place FOK order
pub async fn place_fok_order(
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

**Fill-or-Kill (FOK)**:
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
let order = auth.place_fok_order(
    &token_id,
    OrderSide::Buy,
    "100",      // size
    "0.15"      // price
).await?;
```

---

### `src/data/chainlink.rs`

Polygon RPC configuration for on-chain operations.

#### Key Functions

**`rpc_url()`** - Get appropriate RPC endpoint
```rust
pub fn rpc_url(mode: TradingMode) -> String
```

**Priority**:
1. `ALCHEMY_KEY` env var set → Use Alchemy (higher reliability)
2. Fallback → Public Polygon RPC

---

## Pipeline Layer

### `src/pipeline/price_source.rs`

Unified price source abstraction for multi-exchange support.

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
    Coinbase(Arc<CoinbaseClient>),
}
```

**`PriceTick`** - Normalized price tick
```rust
pub struct PriceTick {
    pub price: f64,
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
pub async fn latest(&self) -> Option<f64>

// Get price history
pub async fn history(&self) -> Vec<PriceTick>

// Get last tick timestamp
pub async fn last_tick_ms(&self) -> Option<i64>
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

#### Usage Example

```rust
let price_source = Arc::new(PriceSource::new(
    PriceSourceType::Binance,
    "BTCUSDT",
    1000,  // buffer size
));

// Start WebSocket
price_source.clone().start().await;

// Query prices
if let Some(price) = price_source.latest().await {
    println!("Current BTC: ${}", price);
}

let history = price_source.history().await;
println!("Last {} prices", history.len());
```

---

### `src/pipeline/signal.rs`

Signal detection module for extreme market sentiment.

#### Key Structures

**`Signal`** - Detected market signal
```rust
pub enum Signal {
    Up,    // Market extremely bearish, buy UP
    Down,  // Market extremely bullish, buy DOWN
    None,  // Market balanced, no trade
}
```

**`SignalComputer`** - Signal computation logic
```rust
pub struct SignalComputer {
    extreme_threshold: Decimal,
    fair_value: Decimal,
}
```

#### Signal Detection Algorithm

```rust
fn compute_signal(&self, yes_price: Decimal, no_price: Decimal) -> Signal {
    let total = yes_price + no_price;
    let mkt_up = yes_price / total;
    
    if mkt_up > self.extreme_threshold {
        Signal::Down  // Market bullish → buy cheap DOWN
    } else if mkt_up < (Decimal::ONE - self.extreme_threshold) {
        Signal::Up    // Market bearish → buy cheap UP
    } else {
        Signal::None  // Balanced
    }
}
```

#### Pre-Filter Checks

Before signal computation:
1. Price buffer has ≥60 samples
2. Latest tick is <30 seconds old
3. Market tokens discovered
4. ≥30 seconds until settlement
5. Market data available (yes/no prices > 0.01)

#### Usage Example

```rust
let computer = SignalComputer::new(
    decimal("0.80"),  // extreme_threshold
    decimal("0.50"),  // fair_value
);

let signal = computer.compute_signal(
    decimal("0.85"),  // yes price
    decimal("0.15"),  // no price
);

match signal {
    Signal::Up => println!("Signal: Buy UP"),
    Signal::Down => println!("Signal: Buy DOWN"),
    Signal::None => println!("No signal"),
}
```

---

### `src/pipeline/decider.rs`

Trade decision logic and risk management.

#### Key Structures

**`AccountState`** - Account tracking
```rust
pub struct AccountState {
    pub balance: Decimal,
    pub consecutive_losses: u32,
    pub consecutive_wins: u32,
    pub daily_pnl: Decimal,
    pub last_trade_time_ms: i64,
    pub last_traded_settlement_ms: i64,
}
```

**`DeciderConfig`** - Decision parameters
```rust
pub struct DeciderConfig {
    pub edge_threshold: Decimal,
    pub cooldown_ms: i64,
    pub extreme_threshold: Decimal,
    pub fair_value: Decimal,
    pub max_daily_loss_pct: Decimal,
    pub momentum_threshold: Decimal,
    pub momentum_lookback_ms: i64,
    pub enforce_limits: bool,
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
    },
}
```

#### Decision Pipeline

```
decide()
├── 1. Already traded? → Pass("already_traded")
├── 2. Risk controls → Pass if enforce_limits=true and violated
├── 3. Market data valid? → Pass("no_market_data")
├── 4. Edge > threshold? → Pass("edge_X%<Y%")
├── 5. Momentum check? → Pass("against_trend")
├── 6. Balance > 0? → Pass("insufficient_balance")
└── TRADE: Calculate position size
```

#### Risk Control Logic

**Advisory Mode** (`enforce_limits: false`):
```rust
fn check_risk_controls(&self, cfg: &DeciderConfig) -> Option<&'static str> {
    if cooldown_active {
        tracing::info!("[RISK] Cooldown... trading continues");
    }
    if daily_loss_exceeded {
        tracing::warn!("[RISK] Daily loss... trading continues");
    }
    None  // Never blocks
}
```

**Strict Mode** (`enforce_limits: true`):
```rust
fn check_risk_controls(&self, cfg: &DeciderConfig) -> Option<&'static str> {
    if cooldown_active {
        tracing::warn!("[RISK] Cooldown... blocking trade");
        return Some("cooldown");
    }
    if daily_loss_exceeded {
        tracing::error!("[RISK] Daily loss... blocking trade");
        return Some("daily_loss_limit");
    }
    None
}
```

#### Position Sizing

```rust
fn calculate_position_size(balance: Decimal) -> Decimal {
    // 1% of balance, minimum $1
    (balance / 100).max(decimal("1"))
}
```

#### Usage Example

```rust
let account = AccountState::new(decimal("1000"));
let cfg = DeciderConfig::default();

let decision = decide(
    Some(decimal("0.85")),  // yes price
    Some(decimal("0.15")),  // no price
    settlement_ms,
    &account,
    &cfg,
    &btc_prices,  // [(price, timestamp), ...]
);

match decision {
    Decision::Trade { direction, size_usdc, edge } => {
        println!("Trade: {:?} ${} (edge: {})", direction, size_usdc, edge);
    }
    Decision::Pass(reason) => {
        println!("No trade: {}", reason);
    }
}
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
    pub btc_price: f64,
}
```

**`PaperOrder`** - Simulated order result
```rust
pub struct PaperOrder {
    pub order_id: String,
    pub filled_shares: Decimal,
    pub cost: Decimal,
}
```

#### Execution Modes

**Paper Mode**:
```rust
fn execute_paper(&self, ctx: &ExecuteContext<'_>) -> Option<PaperOrder> {
    // Generate UUID
    let order_id = Uuid::new_v4().to_string();
    
    // Calculate shares
    let shares = calculate_shares(ctx);
    if shares == 0 {
        return None;  // Zero-share guard
    }
    
    Some(PaperOrder { order_id, filled_shares: shares, cost })
}
```

**Live Mode**:
```rust
async fn execute_live(&self, ctx: &ExecuteContext<'_>) -> Option<LiveOrder> {
    // Get authenticated client
    let client = self.auth_client.as_ref()?;
    
    // Calculate shares
    let shares = calculate_shares(ctx);
    if shares == 0 {
        return None;
    }
    
    // Place FOK order
    let response = client.place_fok_order(
        &token_id,
        OrderSide::Buy,
        &shares.to_string(),
        &price.to_string(),
    ).await.ok()?;
    
    Some(LiveOrder { ... })
}
```

#### Zero-Share Guard

```rust
fn calculate_shares(size_usdc: Decimal, price: Decimal) -> Option<Decimal> {
    let shares = (size_usdc / price).floor();
    if shares > Decimal::ZERO {
        Some(shares)
    } else {
        tracing::warn!("Zero shares calculated, rejecting order");
        None
    }
}
```

#### Usage Example

```rust
let executor = Executor::new(TradingMode::Paper, None);

let ctx = ExecuteContext {
    decision: &decision,
    token_yes: &market.token_yes,
    token_no: &market.token_no,
    poly_yes: Some(decimal("0.15")),
    poly_no: Some(decimal("0.85")),
    settlement_time_ms,
    btc_price: 50000.0,
};

if let Some(order) = executor.execute(&ctx).await {
    println!("Executed: {} shares for ${}", 
        order.filled_shares, 
        order.cost
    );
}
```

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
// Add new position (prevents duplicates)
pub fn add_position(&mut self, pos: PendingPosition)

// Get positions ready for settlement
pub fn due_positions(&self) -> Vec<PendingPosition>

// Settle positions by market slug
pub fn settle_by_slug(&mut self, slug: &str, won: bool) -> Option<SettlementResult>

// Restore positions from state (deduplicates)
pub fn restore_positions(&mut self, positions: Vec<PendingPosition>)
```

#### Settlement Logic

```rust
fn settle(&mut self, position: PendingPosition, won: bool) -> SettlementResult {
    let payout = if won {
        position.filled_shares  // Each share pays $1
    } else {
        Decimal::ZERO
    };
    
    let pnl = payout - position.cost;
    
    SettlementResult {
        payout,
        pnl,
        won,
        ...
    }
}
```

#### Duplicate Prevention

```rust
fn add_position(&mut self, pos: PendingPosition) {
    // Check for existing position with same condition_id
    if self.pending.iter().any(|p| p.condition_id == pos.condition_id) {
        tracing::warn!("Duplicate position, skipping");
        return;
    }
    self.pending.push_back(pos);
}
```

#### Position Combining

When multiple positions exist for same market:
```rust
fn combine_positions(&self, positions: Vec<PendingPosition>) -> PendingPosition {
    PendingPosition {
        size_usdc: positions.iter().map(|p| p.size_usdc).sum(),
        filled_shares: positions.iter().map(|p| p.filled_shares).sum(),
        cost: positions.iter().map(|p| p.cost).sum(),
        // Use first position for other fields
        ...
    }
}
```

#### Usage Example

```rust
let mut settler = Settler::new();

// Add position
settler.add_position(PendingPosition {
    direction: Direction::Up,
    size_usdc: decimal("10"),
    entry_price: decimal("0.15"),
    filled_shares: decimal("66"),
    cost: decimal("9.9"),
    settlement_time_ms,
    condition_id: "0xabc...".to_string(),
    market_slug: "btc-updown-5m-123".to_string(),
});

// Check for settled positions
if market_resolved {
    let result = settler.settle_by_slug(&slug, won).unwrap();
    println!("Settled: PnL = ${}", result.pnl);
}
```
