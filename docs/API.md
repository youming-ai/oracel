# API Documentation

## Overview

This document describes the internal APIs and data structures used throughout the Polymarket 5m Bot.

---

## Configuration API

### `Config`

Root configuration structure.

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
}
```

#### Methods

**`load(path: &Path) -> Result<Self>`**
- Loads configuration from JSON file
- Returns error if file not found or invalid JSON

**`save(&self, path: &Path) -> Result<()>`**
- Saves configuration to JSON file
- Pretty-printed JSON format

**`validate(&self) -> Result<()>`**
- Validates all configuration values
- Returns error with descriptive message if invalid

#### Example

```rust
use std::path::Path;

// Load configuration
let config = Config::load(Path::new("config.json"))?;

// Validate
config.validate()?;

// Access settings
println!("Mode: {:?}", config.trading.mode);
```

---

## Price Source API

### `PriceSource`

Unified price source abstraction.

```rust
pub struct PriceSource {
    client: PriceClient,
    buffer: Arc<RwLock<VecDeque<PriceTick>>>,
    max: usize,
    started: AtomicBool,
}
```

#### Methods

**`new(source_type: PriceSourceType, symbol: &str, max: usize) -> Self`**
- Creates new price source
- `source_type`: Binance, Coinbase, etc.
- `symbol`: Trading pair symbol
- `max`: Buffer size (number of ticks to retain)

**`start(self: Arc<Self>)`**
- Starts WebSocket connections
- Spawns client and consumer tasks
- Idempotent (safe to call multiple times)

**`latest(&self) -> Option<Decimal>`**
- Returns most recent price
- Async method, requires `.await`
- Returns `None` if buffer empty

**`last_tick_ms(&self) -> Option<i64>`**
- Returns timestamp of most recent tick
- Async method, requires `.await`

**`buffer_len(&self) -> usize`**
- Returns number of ticks currently in the buffer
- Async method, requires `.await`

#### Example

```rust
use std::sync::Arc;

let price_source = Arc::new(PriceSource::new(
    PriceSourceType::Binance,
    "BTCUSDT",
    1000,
));

// Start WebSocket
price_source.clone().start().await;

// Query price
if let Some(price) = price_source.latest().await {
    println!("BTC: ${}", price);
}

// Get buffer size
let len = price_source.buffer_len().await;
println!("Buffer: {} ticks", len);
```

---

## Trading Decision API

### `decide()`

Main trading decision function.

```rust
pub fn decide(
    ctx: &DecideContext,
    account: &AccountState,
    cfg: &DeciderConfig,
) -> Decision
```

#### Parameters

| Parameter | Type | Description |
|-----------|------|-------------|
| `ctx` | `&DecideContext` | Market context (yes/no prices, remaining_ms) |
| `account` | `&AccountState` | Current account state |
| `cfg` | `&DeciderConfig` | Decision configuration |

#### Returns

**`Decision::Trade { direction, size_usdc, edge }`**
- Trade should be executed
- `direction`: Up or Down
- `size_usdc`: Position size in USDC
- `edge`: Calculated edge (0.0-1.0)

**`Decision::Pass(reason)`**
- Trade should not be executed
- `reason`: String explaining why

#### Pass Reasons

| Reason | Meaning |
|--------|---------|
| `"insufficient_balance"` | Account balance ≤ 0 |
| `"no_market_data"` | Missing or invalid market prices |
| `"no_liquidity"` | Zero or negative total liquidity |
| `"not_extreme_XX%"` | Market not extreme enough |
| `"edge_X%<Y%"` | Edge below threshold |

#### Example

```rust
let decision = decide(
    &DecideContext {
        market_yes: Some(decimal("0.85")),
        market_no: Some(decimal("0.15")),
        remaining_ms: 120000,
    },
    &account,
    &config,
);

match decision {
    Decision::Trade { direction, size_usdc, edge } => {
        println!("Trade: {:?} ${} (edge: {}%)", 
            direction, size_usdc, edge * 100);
    }
    Decision::Pass(reason) => {
        println!("No trade: {}", reason);
    }
}
```

---

## Account State API

### `AccountState`

Tracks trading account information.

```rust
pub struct AccountState {
    pub balance: Decimal,
    pub initial_balance: Decimal,
    pub consecutive_losses: u32,
    pub consecutive_wins: u32,
    pub total_losses: u32,
    pub total_wins: u32,
    pub daily_pnl: Decimal,
    pub daily_reset_date: String,
}
```

#### Methods

**`new(balance: Decimal) -> Self`**
- Creates new account with initial balance

**`record_trade(&mut self, cost: Decimal)`**
- Records a trade execution
- Deducts cost from balance
- Updates last trade time

**`record_settlement(&mut self, result: &SettlementResult, max_consecutive_losses: u32)`**
- Records a settlement outcome
- Updates balance with payout
- Tracks win/loss streaks
- Logs risk warnings

**`already_traded_market(&self, settlement_ms: i64) -> bool`**
- Checks if already traded this window

**`check_daily_reset(&mut self)`**
- Resets daily PnL at midnight UTC

#### Example

```rust
let mut account = AccountState::new(decimal("1000"));

// Record trade
account.record_trade(decimal("10"));

// Record settlement
let result = SettlementResult {
    direction: Direction::Up,
    payout: decimal("50"),
    pnl: decimal("40"),
    won: true,
    condition_id: "0x...".to_string(),
    entry_btc_price: decimal("50000"),
};
account.record_settlement(&result, 8);

println!("Balance: ${}", account.balance);
println!("Daily PnL: ${}", account.daily_pnl);
```

---

## Order Execution API

### `Executor`

Handles order execution for paper and live modes.

```rust
pub struct Executor {
    mode: TradingMode,
    auth_client: Option<AuthenticatedPolyClient>,
}
```

#### Methods

**`new(mode: TradingMode, auth_client: Option<AuthenticatedPolyClient>) -> Self`**
- Creates new executor
- `auth_client` required for live mode

**`execute(&self, ctx: &ExecuteContext<'_>) -> Option<impl Order>`**
- Executes a trade decision
- Returns order details if successful
- Returns `None` if execution failed or skipped

#### ExecuteContext

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

#### Example

```rust
let executor = Executor::new(TradingMode::Paper, None);

let ctx = ExecuteContext {
    decision: &decision,
    token_yes: &market.token_yes,
    token_no: &market.token_no,
    poly_yes: Some(decimal("0.15")),
    poly_no: Some(decimal("0.85")),
    settlement_time_ms,
    btc_price: decimal("50000"),
};

if let Some(order) = executor.execute(&ctx).await {
    println!("Order: {} shares, cost: ${}", 
        order.filled_shares, 
        order.cost);
}
```

---

## Settlement API

### `Settler`

Manages pending positions and settlements.

```rust
pub struct Settler {
    pending: VecDeque<PendingPosition>,
}
```

#### Methods

**`new() -> Self`**
- Creates new settler with empty pending queue

**`add_position(&mut self, pos: PendingPosition)`**
- Adds new position to pending queue
- Prevents duplicates (by condition_id)

**`due_positions(&self) -> Vec<PendingPosition>`**
- Returns positions past settlement time

**`settle_by_slug(&mut self, slug: &str, won: bool) -> Option<SettlementResult>`**
- Settles all positions for given market slug
- Combines multiple positions if present
- Returns settlement result

**`pending_count(&self) -> usize`**
- Returns number of pending positions

#### PendingPosition

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

#### Example

```rust
let mut settler = Settler::new();

// Add position
settler.add_position(PendingPosition {
    direction: Direction::Up,
    size_usdc: decimal("10"),
    entry_price: decimal("0.15"),
    filled_shares: decimal("66"),
    cost: decimal("9.9"),
    settlement_time_ms: 1704067200000,
    condition_id: "0xabc...".to_string(),
    market_slug: "btc-updown-5m-123".to_string(),
});

// Check for settled positions
let due = settler.due_positions();
println!("Due for settlement: {}", due.len());

// Settle
if let Some(result) = settler.settle_by_slug("btc-updown-5m-123", true) {
    println!("Payout: ${}, PnL: ${}", result.payout, result.pnl);
}
```

---

## Market Discovery API

### `MarketDiscovery`

Gamma API client for market discovery.

```rust
pub struct MarketDiscovery {
    gamma_api_url: String,
}
```

#### Methods

**`new(config: DiscoveryConfig) -> Self`**
- Creates new discovery client

**`discover() -> Result<ActiveMarket>`**
- Finds currently active 5-minute market
- Returns market info

**`fetch_market_by_slug(slug: &str) -> Result<GammaMarket>`**
- Fetches a specific market by slug
- Returns market data

#### ActiveMarket

```rust
pub struct ActiveMarket {
    pub condition_id: String,
    pub market_slug: String,
    pub token_yes: String,
    pub token_no: String,
    pub settlement_time: DateTime<Utc>,
}
```

#### ResolutionState

```rust
pub struct ResolutionState {
    pub resolved: bool,
    pub outcome: Option<Direction>,  // Some(Up) or Some(Down) if resolved
}
```

#### Example

```rust
let discovery = MarketDiscovery::new(DiscoveryConfig {
    gamma_api_url: "https://gamma-api.polymarket.com".to_string(),
});

// Find active market
if let Ok(market) = discovery.discover().await {
    println!("Active: {} settling at {}", 
        market.market_slug, 
        market.settlement_time);
}

// Fetch market by slug
let gamma_market = discovery.fetch_market_by_slug("btc-updown-5m-1704067200").await?;
```

---

## Data Types

### Core Enums

#### `TradingMode`

```rust
pub enum TradingMode {
    Paper,  // Simulated trading
    Live,   // Real trading
}

impl TradingMode {
    pub fn is_paper(self) -> bool;
    pub fn is_live(self) -> bool;
}
```

#### `Direction`

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Direction {
    Up,
    Down,
}

impl Direction {
    pub fn as_str(&self) -> &'static str;  // "UP" or "DOWN"
}
```

#### `PriceSourceType`

```rust
pub enum PriceSourceType {
    Binance,
    BinanceWs,
    Coinbase,
    CoinbaseWs,
}

impl PriceSourceType {
    pub fn expects_dash_symbol(self) -> bool;  // true for Coinbase
}
```

### Core Structs

#### `PriceTick`

```rust
#[derive(Debug, Clone, Copy)]
pub struct PriceTick {
    pub price: Decimal,
    pub timestamp_ms: i64,
}
```

#### `SettlementResult`

```rust
pub struct SettlementResult {
    pub direction: Direction,
    pub payout: Decimal,
    pub pnl: Decimal,
    pub won: bool,
    pub condition_id: String,
    pub entry_btc_price: Decimal,
}
```

#### `DeciderConfig`

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

---

## Error Types

### Common Error Patterns

#### Configuration Errors

```rust
// Validation error
Err(anyhow!("strategy.extreme_threshold must be in (0, 1)"))

// Invalid symbol format
Err(anyhow!("price_source.symbol must match Binance format like BTCUSDT when source=binance"))
```

#### Network Errors

```rust
// Timeout
tokio::time::timeout(Duration::from_secs(10), operation)
    .await
    .map_err(|_| anyhow!("Operation timed out"))?
```

#### Trading Errors

```rust
// Insufficient balance
Decision::Pass("insufficient_balance".into())

// Execution failed
tracing::warn!("Order execution failed: {}", error);
```

---

## Logging API

### Log Levels

| Level | Usage |
|-------|-------|
| `ERROR` | Failures that stop trading or require intervention |
| `WARN` | Risk warnings, unexpected conditions |
| `INFO` | Normal operations, trades, settlements |
| `DEBUG` | Detailed diagnostics (price updates, internal state) |
| `TRACE` | Very verbose (WebSocket messages, raw data) |

### Log Prefixes

| Prefix | Meaning |
|--------|---------|
| `[INIT]` | Initialization and startup |
| `[MKT]` | Market discovery updates |
| `[IDLE]` | Pre-signal filter skip reasons |
| `[SKIP]` | Decision pipeline skip reasons |
| `[TRADE]` | Trade execution |
| `[SETTLED]` | Position settlement |
| `[STATUS]` | Periodic status summary |
| `[RISK]` | Risk control warnings |
| `[WS]` | WebSocket connection events |
| `[BAL]` | Balance update events |

### Custom Log Messages

```rust
use tracing::{info, warn, error, debug};

// Informational
info!("[TRADE] {} @ {:.3} edge={:.0}%", direction, price, edge * 100);

// Warning
warn!("[RISK] Daily loss limit reached: pnl={:.2}", daily_pnl);

// Error
error!("[WS] Binance connection failed: {}", error);

// Debug
debug!("Price update: {} @ {}", price, timestamp);
```

---

## Best Practices

### API Usage Guidelines

1. **Always validate configuration** before starting trading
2. **Check for None** when querying prices or orders
3. **Handle all Decision variants** in match statements
4. **Log appropriately** at the right level
5. **Use Decimal for money** never f64
6. **Clone account state** before passing to decide()
7. **Check errors** from async operations

### Example: Complete Trading Loop

```rust
async fn trading_tick(&mut self) -> Result<()> {
    // 1. Get market data
    let market = self.discovery.discover().await?;
    let market = match market {
    
    // 2. Get prices
    let (yes_price, no_price) = self.get_market_prices(&market).await?;
    
    // 3. Make decision
    let account = self.account.read().await.clone();
    let remaining_ms = market.settlement_time.timestamp_millis() - now_ms();
    let decision = decide(
        &DecideContext {
            market_yes: yes_price,
            market_no: no_price,
            remaining_ms,
        },
        &account,
        &self.decider_config,
    );
    
    // 4. Execute if trade
    if let Decision::Trade { .. } = decision {
        let ctx = ExecuteContext {
            decision: &decision,
            token_yes: &market.token_yes,
            token_no: &market.token_no,
            poly_yes: yes_price,
            poly_no: no_price,
            settlement_time_ms: market.settlement_time.timestamp_millis(),
            btc_price: self.price_source.latest().await.unwrap_or_default(),
        };
        
        if let Some(order) = self.executor.execute(&ctx).await {
            // Record position
            self.settler.write().await.add_position(PendingPosition {
                direction: order.direction,
                size_usdc: order.size_usdc,
                entry_price: order.price,
                filled_shares: order.filled_shares,
                cost: order.cost,
                settlement_time_ms: market.settlement_time.timestamp_millis(),
                condition_id: market.condition_id.clone(),
                market_slug: market.market_slug.clone(),
            });
        }
    }
    
    Ok(())
}
```
