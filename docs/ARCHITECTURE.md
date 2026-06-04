# Architecture Overview

## System Architecture

The Polymarket 5m Bot follows a pipeline architecture with clear separation of concerns:

```
┌─────────────────────────────────────────────────────────────┐
│                    Data Sources                             │
├────────────────────┬────────────────────────────────────────┤
│     Binance        │      Polymarket Gamma API              │
│   WebSocket        │         (REST API)                     │
└─────────┬──────────┴──────────────────┬─────────────────────┘
          │                             │
          └──────────┬──────────────────┘
                     │
┌────────────────────▼─────────────────────────────────────┐
│                   Pipeline                                │
│  ┌──────────────┬──────────────────────────┬──────────┐  │
│  │ PriceSource  │        Decider           │ Executor │  │
│  │   (Stage 1)  │  (Stage 2: Signal+Decide)│(Stage 3) │  │
│  └──────────────┴──────────────────────────┴──────────┘  │
│                           │                               │
│                      ┌────▼────┐                          │
│                      │ Settler │                          │
│                      │(Stage 4)│                          │
│                      └─────────┘                          │
└───────────────────────────────────────────────────────────┘
          │
          ▼
┌─────────────────────────────────────┐
│       ratatui TUI Dashboard         │
│   (Arc<RwLock<TuiState>> shared)    │
└─────────────────────────────────────┘
```

## Pipeline Stages

### Stage 1: PriceSource
**Purpose**: Real-time BTC price ingestion from Binance WebSocket

**Key Responsibilities**:
- WebSocket connection management with automatic reconnection
- Price buffer maintenance (rolling window of last N ticks)
- Exchange timestamp tracking for accurate staleness detection

**Performance Characteristics**:
- Lock-free read path for latest price queries
- Zero-allocation hot path for price updates
- <1ms ingestion latency target

### Stage 2: Decider (Signal + Decision)
**Purpose**: Market opportunity detection and trade decision logic

**Key Responsibilities**:
- Fetch Polymarket CLOB quotes (yes/no mid prices)
- Detect extreme market sentiment (yes > 0.90 → Down, no > 0.90 → Up)
- Orderbook spread check (reject if yes+no spread > 6%)
- BTC trend momentum confirmation
- Entry price range validation
- TTL (time-to-live) minimum check
- Balance and daily loss limit enforcement
- Sliding-window circuit breaker (win rate check)

**Decision Pipeline**:
```
decide()
├── 1. Balance > 0? → Pass("insufficient_balance")
├── 2. Daily loss limit? → Pass("daily_loss_limit")
├── 3. Market data valid? → Pass("no_market_data")
├── 4. Extreme market? → Pass("not_extreme_XX%")
├── 5. Spread check? → Pass("spread_too_wide")
├── 6. Entry price range? → Pass("price_out_of_range")
├── 7. Min TTL for entry? → Pass("ttl_below_entry_floor")
├── 8. BTC trend against? → Pass("btc_trend_against_XX%")
├── 9. Circuit breaker? → Pass("circuit_breaker_wr_XX%")
└── TRADE: Calculate position size, edge, payoff ratio
```

### Stage 3: Executor
**Purpose**: Order execution via Polymarket CLOB

**Key Responsibilities**:
- Place FAK (Fill-And-Kill) limit orders via CLOB
- Slippage tolerance application (1% default)
- Zero-share order rejection
- Order ID safe handling (prevent slicing panics)
- FAK retry logic on failure

### Stage 4: Settler
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
│ - Execute order via CLOB                │
│ - Record position in settler            │
└─────────────────────────────────────────┘
     │
     ▼
┌─────────────────────────────────────────┐
│   TUI State Update                      │
│ - BTC price, market info                │
│ - Balance, PnL, stats                   │
│ - Trade history, decision status        │
└─────────────────────────────────────────┘
```

## Module Organization

```
src/
├── main.rs                  # Entry point, tracing setup, TUI init
├── bot.rs                   # Bot struct, main loop, order logic, trade recording
├── config.rs                # Configuration definitions, validation, defaults
├── state.rs                 # BotState (in-memory: idle reasons, FAK state)
├── tasks.rs                 # Background tasks: settlement, market refresh, status, balance
├── lib.rs                   # Library re-exports (config, data, pipeline, tui)
├── cli.rs                   # CLI tools binary (polybot-tools)
│
├── data/                    # External data source clients
│   ├── mod.rs               # Data module exports
│   ├── binance.rs           # Binance WebSocket client
│   ├── market_discovery.rs  # Gamma API integration
│   └── polymarket.rs        # Polymarket CLOB client + RPC URL
│
├── pipeline/                # Trading pipeline stages
│   ├── mod.rs               # Pipeline module exports
│   ├── price_source.rs      # Stage 1: Price ingestion
│   ├── decider.rs           # Stage 2: Signal detection + trade decision
│   ├── executor.rs          # Stage 3: Order execution
│   ├── settler.rs           # Stage 4: Settlement
│   └── test_helpers.rs      # Test utilities (d() helper)
│
└── tui/                     # Terminal dashboard
    ├── mod.rs               # TUI event loop
    ├── state.rs             # TuiState (shared via Arc<RwLock>)
    ├── ui.rs                # Layout and widget rendering
    ├── event.rs             # Event handling stubs
    └── keys.rs              # Key handling stubs
```

## Concurrency Model

### Async Task Structure
```
Main Task
├── PriceSource Task
│   └── WebSocket Client Task (reconnect loop)
│   └── Price Consumer Task (buffer updates)
├── Settlement Checker Task (15s interval)
├── Market Refresher Task (60s interval)
├── Signal Tick Task (1s interval)
│   └── Decision → Execution → Settlement
└── TUI Blocking Thread
    └── 250ms render loop (reads TuiState via try_read)
```

### Synchronization Primitives
- **RwLock**: Used for shared state (balance, positions, market data, TUI state)
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
    tui_state: Arc<RwLock<TuiState>>,
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
- Symbol format validation

### Safe Defaults
- Fixed position sizing for simplicity
- Balance-based trade rejection
