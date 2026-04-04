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
    pub timeouts: TimeoutsConfig,
    pub redeem: RedeemConfig,
    pub misc: MiscConfig,
    pub time_windows: TimeWindowsConfig,
}
```

#### Methods

**`load(path: &Path) -> Result<Self>`**
- Loads configuration from TOML file
- Returns error if file not found or invalid TOML

**`save(&self, path: &Path) -> Result<()>`**
- Saves configuration to TOML file

**`validate(&self) -> Result<()>`**
- Validates all configuration values
- Returns error with descriptive message if invalid

#### Example

```rust
use std::path::Path;

// Load configuration
let config = Config::load(Path::new("config.toml"))?;

// Validate
config.validate()?;

// Access settings
println!("Mode: {:?}", config.trading.mode);
```

---

## Price Source API

### `PriceSource`

Price source abstraction for Binance WebSocket feeds.

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
- `source_type`: Binance
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
```

---

## Trading Decision API

### `decide()`

Main trading decision function (includes signal detection).

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

**`Decision::Trade { direction, size_usdc, edge, payoff_ratio }`**
- Trade should be executed
- `direction`: Up or Down
- `size_usdc`: Position size in USDC
- `edge`: Calculated edge (0.0-1.0)
- `payoff_ratio`: (1 - cheap_price) / cheap_price

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
| `"entry_price_out_of_range"` | Entry price outside min/max bounds |
| `"ttl_too_short"` | Insufficient time remaining |
| `"daily_loss_limit"` | Daily loss limit exceeded |

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

**`record_settlement(&mut self, result: &SettlementResult)`**
- Records a settlement outcome
- Updates balance with payout
- Tracks win/loss streaks

**`already_traded_market(&self, settlement_ms: i64) -> bool`**
- Checks if already traded this window

**`check_daily_reset(&mut self)`**
- Resets daily PnL at midnight UTC

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

**`fetch_market_by_slug(slug: &str) -> Result<GammaMarket>`**
- Fetches a specific market by slug

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
    pub outcome: Option<Direction>,
}
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

## Logging API

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
