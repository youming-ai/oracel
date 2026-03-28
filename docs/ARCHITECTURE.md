# Architecture Overview

## System Architecture

The Polymarket 5m Bot follows a pipeline architecture with clear separation of concerns:

```
┌─────────────────────────────────────────────────────────────┐
│                    Data Sources                             │
├──────────────┬──────────────┬───────────────────────────────┤
│   Binance    │   Coinbase   │      Polymarket Gamma API     │
│  WebSocket   │  WebSocket   │         (REST API)            │
└──────┬───────┴──────┬───────┴───────────────┬───────────────┘
       │              │                       │
       └──────────────┼───────────────────────┘
                      │
┌─────────────────────▼─────────────────────────────────────┐
│                   Pipeline                                │
│  ┌──────────────┬──────────────┬──────────────┬──────────┐│
│  │ PriceSource  │    Signal    │   Decider    │ Executor ││
│  │   (Stage 1)  │   (Stage 2)  │  (Stage 3)   │(Stage 4) ││
│  └──────────────┴──────────────┴──────────────┴──────────┘│
│                           │                               │
│                      ┌────▼────┐                          │
│                      │ Settler │                          │
│                      │(Stage 5)│                          │
│                      └─────────┘                          │
└───────────────────────────────────────────────────────────┘
                      │
       ┌──────────────┼──────────────┐
       │              │              │
┌──────▼──────┐ ┌────▼─────┐ ┌─────▼──────┐
│  Paper Mode │ │ Live Mode│
│  (Simulated)│ │(Real CLOB)│
└─────────────┘ └──────────┘
```

## Pipeline Stages

### Stage 1: PriceSource
**Purpose**: Real-time BTC price ingestion from multiple exchanges

**Key Responsibilities**:
- WebSocket connection management with automatic reconnection
- Price buffer maintenance (rolling window of last N ticks)
- Exchange timestamp tracking for accurate staleness detection
- Multi-exchange support (Binance, Coinbase) via enum dispatch

**Performance Characteristics**:
- Lock-free read path for latest price queries
- Zero-allocation hot path for price updates
- <1ms ingestion latency target

### Stage 2: Signal
**Purpose**: Market opportunity detection

**Key Responsibilities**:
- Fetch Polymarket CLOB quotes (yes/no mid prices)
- Calculate market bias and detect extreme sentiment
- Filter out non-extreme markets (pre-filter before expensive decision logic)
- Validate market data quality (non-zero prices, sufficient liquidity)

**Decision Logic**:
```rust
if market_bias > extreme_threshold (0.95) → Signal::Down
if market_bias < 1 - extreme_threshold (0.05) → Signal::Up
else → no signal (balanced market)
```

### Stage 3: Decider
**Purpose**: Trade decision logic

**Key Responsibilities**:
- One-trade-per-window enforcement
- Balance check (reject if ≤ 0)
- Edge calculation: `edge = fair_value - cheap_side_price`
- Position sizing calculation
- Market data validation

**Decision Pipeline**:
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

### Stage 4: Executor
**Purpose**: Order execution (paper or live)

**Key Responsibilities**:
- Paper mode: Generate simulated orders with UUID tracking
- Live mode: Place FAK (Fill-And-Kill) limit orders via CLOB
- Zero-share order rejection
- Order ID safe handling (prevent slicing panics)
- FAK retry logic on failure

### Stage 5: Settler
**Purpose**: Position settlement and PnL calculation

**Key Responsibilities**:
- Track pending positions until settlement time
- Poll Gamma API for market resolution
- Calculate payouts and PnL
- Prevent duplicate position tracking
- Handle position combining for multiple orders on same market

## Data Flow

```
┌────────────────────────────────────────────────────────────┐
│                     Main Event Loop                         │
│                    (1-second intervals)                     │
└────────────────────┬───────────────────────────────────────┘
                     │
    ┌────────────────┼────────────────┐
    │                │                │
    ▼                ▼                ▼
┌─────────┐    ┌──────────┐    ┌──────────┐
│ Price   │    │ Settlement│   │  Market  │
│ Update  │    │  Check    │   │ Refresh  │
│ (1s)    │    │  (15s)    │   │  (60s)   │
└────┬────┘    └──────────┘    └──────────┘
     │
     ▼
┌─────────────────────────────────────────┐
│ 1. Check Price Buffer (≥60 samples)     │
│ 2. Check Price Staleness (<30s old)     │
│ 3. Check Market Readiness               │
│ 4. Check Time-to-Live (≥30s remaining)  │
│ 5. Signal Detection (extreme market?)   │
└─────────────────────────────────────────┘
     │
     ▼
┌─────────────────────────────────────────┐
│           Decision Pipeline             │
│ - Market data valid?                    │
│ - Spread check?                         │
│ - Extreme market?                       │
│ - Entry price in range?                 │
│ - TTL sufficient for entry?             │
│ - Balance > 0?                          │
│ - Daily loss limit?                     │
└─────────────────────────────────────────┘
     │
     ▼
┌─────────────────────────────────────────┐
│         Trade Execution                 │
│ - Calculate position size               │
│ - Validate shares > 0                   │
│ - Execute order (paper/live)            │
│ - Record position in settler            │
└─────────────────────────────────────────┘
```

## Module Organization

```
src/
├── main.rs                  # Entry point, tracing setup, CLI
├── bot.rs                   # Bot struct, main loop, order logic, trade recording
├── config.rs                # Configuration definitions, validation, defaults
├── state.rs                 # BotState (in-memory: idle reasons, FAK state)
├── tasks.rs                 # Background tasks: settlement, market refresh, status, balance
├── lib.rs                   # Library re-exports (config, data, pipeline)
├── cli.rs                   # CLI tools binary (polybot-tools)
│
├── data/                    # External data source clients
│   ├── mod.rs               # Data module exports
│   ├── binance.rs          # Binance WebSocket client
│   ├── coinbase.rs         # Coinbase WebSocket client
│   ├── market_discovery.rs # Gamma API integration
│   └── polymarket.rs       # Polymarket CLOB client + RPC URL selection
│
└── pipeline/               # Trading pipeline stages
    ├── mod.rs              # Pipeline module exports
    ├── price_source.rs     # Stage 1: Price ingestion
    ├── signal.rs           # Stage 2: Signal detection
    ├── decider.rs          # Stage 3: Decision logic
    ├── executor.rs         # Stage 4: Order execution
    └── settler.rs          # Stage 5: Settlement
```

## Concurrency Model

### Async Task Structure
```
Main Task
├── PriceSource Task (per exchange)
│   └── WebSocket Client Task (reconnect loop)
│   └── Price Consumer Task (buffer updates)
├── Settlement Checker Task (15s interval)
├── Market Refresher Task (60s interval)
└── Signal Tick Task (1s interval)
    └── Decision → Execution → Settlement
```

### Synchronization Primitives
- **RwLock**: Used for shared state (balance, positions, market data)
- **broadcast channels**: Price tick distribution from exchange clients
- **AtomicBool**: PriceSource start guard (prevent duplicate starts)

### State Sharing Pattern
```rust
// Shared state wrapped in Arc<RwLock<T>>
struct Bot {
    account: Arc<RwLock<AccountState>>,
    settler: Arc<RwLock<Settler>>,
    market_state: Arc<RwLock<MarketState>>,
    price_source: Arc<PriceSource>,
}
```

## Error Handling Strategy

### Error Categories
1. **Transient Errors**: Network timeouts, temporary API failures
   - Action: Retry with exponential backoff
   - Example: WebSocket disconnections

2. **Permanent Errors**: Invalid configuration, invalid symbols
   - Action: Log error and terminate/fail fast
   - Example: Binance -1121 (invalid symbol)

3. **Business Logic Errors**: Insufficient balance, no extreme signal
   - Action: Skip trade, log reason
   - Example: Balance zero, not extreme market

### Error Propagation
- `anyhow::Result` for top-level error handling
- Custom error types for domain-specific failures
- Structured logging with context (tracing crate)

## Performance Considerations

### Hot Path Optimizations
1. **Lock-free reads**: Latest price accessed via read lock (no contention)
2. **Zero-allocation**: Price updates use pre-allocated buffer
3. **Enum dispatch**: Avoid trait object overhead for price clients

### Memory Management
- Fixed-size price buffer (circular queue, 1000 ticks default)
- Streaming JSON parsing (no full document materialization)
- Efficient decimal arithmetic (rust_decimal crate)

### Resource Limits
- WebSocket buffer: 1000 price ticks
- Broadcast channel: 1000 message backlog
- HTTP timeouts: 10-30 seconds depending on operation

## Security Considerations

### Secret Management
- Private keys stored in `SecretString` (zero-on-drop)
- Environment variable loading from `.env` file
- No secrets in configuration files or logs

### Input Validation
- Configuration validation on startup
- Price range validation (prevent degenerate orders)
- Symbol format validation (exchange-specific)

### Safe Defaults
- Paper mode default (no real trades without explicit opt-in)
- Fixed position sizing for simplicity
- Balance-based trade rejection
