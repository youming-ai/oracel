# Rust Trading Bot Architecture Patterns

> Generated via librarian agent research (2026-03-19)

## 1. Notable Open-Source Rust Trading Bots

### Polymarket-Specific

| Repository | Stars | Description |
|---|---|---|
| [Polymarket/rs-clob-client](https://github.com/Polymarket/rs-clob-client) | 617 | **Official** Polymarket Rust SDK |
| [HyperBuildX/Polymarket-Trading-Bot-Rust](https://github.com/HyperBuildX/Polymarket-Trading-Bot-Rust) | 371 | BTC/ETH/Solana/XRP prediction market bot, copy-trading & arbitrage |
| [bitman09/Rust-Politics-Sports-Polymarket-Trading-Bot](https://github.com/bitman09/Rust-Politics-Sports-Polymarket-Trading-Bot) | 196 | Sports & politics binary markets with trailing stop strategy |
| [haredoggy/Polymarket-Trading-Bot-Toolkits](https://github.com/haredoggy/Polymarket-Trading-Bot-Toolkits) | 186 | High-performance Rust toolkit for Polymarket |
| [PolyScripts/polymarket-5min-15min-1hr-btc-arbitrage-trading-bot-rust](https://github.com/PolyScripts/polymarket-5min-15min-1hr-btc-arbitrage-trading-bot-rust) | 73 | Multi-timeframe BTC arbitrage (5m/15m/1hr) |

### General Rust Trading Bots

| Repository | Stars | Description |
|---|---|---|
| [barter-rs/barter-rs](https://github.com/barter-rs/barter-rs) | 2,000 | **Premier** event-driven trading framework -- live, paper, backtesting |
| [hedge0/trading_bot_rust](https://github.com/hedge0/trading_bot_rust) | 16 | SPX options arbitrage via IBKR API, ~100ms detection latency |
| [Zuytan/rustrade](https://github.com/Zuytan/rustrade) | 3 | Multi-agent architecture, 10 strategies, risk management, egui UI |

## 2. Rust Crate Ecosystem for Trading

### Core Infrastructure

| Crate | Purpose | Notes |
|---|---|---|
| `tokio` | Async runtime | Universal standard for Rust trading bots |
| `reqwest` | HTTP client | REST API calls to exchanges |
| `tokio-tungstenite` | WebSocket | Real-time market data streaming |
| `serde` / `serde_json` | Serialization | JSON parsing for API responses |

### Financial Precision

| Crate | Purpose | Notes |
|---|---|---|
| `rust_decimal` | Fixed-point decimal math | Avoids floating-point precision loss |
| `rust_decimal_macros` | `dec!()` macro | Compile-time decimal literals |
| `chrono` | Date/time | Trade timestamps, candle intervals |

### Polymarket-Specific

| Crate | Purpose | Notes |
|---|---|---|
| `polymarket-client-sdk` (v0.4) | Official Polymarket CLOB client | Features: `clob`, `ws`, `data`, `gamma`, `bridge`, `rfq`, `ctf` |
| `alloy` | Ethereum signing & primitives | Used by Polymarket SDK for wallet auth |

### Market Data & Exchange Integration (barter-rs ecosystem)

| Crate | Purpose |
|---|---|
| `barter-data` | WebSocket market data streaming, normalised tick-by-tick |
| `barter-execution` | Order execution & account streaming |
| `barter-instrument` | Exchange/Instrument/Asset type definitions |

### Observability & Persistence

| Crate | Purpose |
|---|---|
| `tracing` | Structured logging with spans |
| `sqlx` | Async SQL (SQLite) for local persistence |
| `prometheus` | Win rate, drawdown, latency metrics |

## 3. Architecture Patterns

### Pattern A: Event-Driven Engine (barter-rs model)

The dominant pattern in production Rust trading systems:

```
+--------------------------------------------------+
|                   Engine                          |
|  +-----------+  +-----------+  +---------------+ |
|  | Market    |->| Strategy  |->| RiskManager   | |
|  | Data      |  | Signal    |  | Validation    | |
|  | Stream    |  | Generator |  | Pipeline      | |
|  +-----------+  +-----------+  +------+--------+ |
|                                       |           |
|  +-----------+  +-----------+  +------v--------+ |
|  | Audit     |<-| Portfolio |<-| Execution     | |
|  | Stream    |  | State     |  | Client        | |
|  +-----------+  +-----------+  +---------------+ |
+--------------------------------------------------+
```

**Key traits:**
- `SignalGenerator` -- strategy produces signals
- `RiskManager` -- validates/exposes orders
- `ExecutionClient` -- submits orders to exchange
- `MarketUpdater` / `FillUpdater` -- state management
- `OrderGenerator` -- portfolio generates orders

### Pattern B: Multi-Agent Architecture (rustrade model)

6 specialized agents communicating via channels:
- **Sentinel** -- monitors WebSocket connections
- **Scanner** -- discovers "Top Movers" and volatility
- **Analyst** -- regime detection (Bull/Bear/Sideways/Volatile)
- **Risk Manager** -- correlation filters, circuit breakers, PDT protection
- **Order Throttler** -- rate limiting
- **Executor** -- broker API interaction

### Pattern C: Strategy Pattern (Polymarket bots)

```rust
trait ExecutionStrategy {
    fn execute_order(&self, order_id: u32, quantity: u32);
}

struct TwapStrategy;
struct VwapStrategy;
struct PovStrategy { participation_rate: f64 }

struct OrderExecutor {
    strategy: Box<dyn ExecutionStrategy>,
}
```

### Pattern D: Observer Pattern for Market Data

```rust
trait Observer {
    fn update(&self, instrument_id: &str, price: f64);
}

struct MarketDataFeed {
    observers: RefCell<Vec<Rc<dyn Observer>>>,
    price: RefCell<f64>,
}
```

## 4. Configuration, Logging & Error Recovery

### Configuration
- **Environment variables** (`.env` files) -- dominant for API keys and risk parameters
- **TOML/JSON config files** -- strategy parameters
- **Type-level state machines** -- Polymarket SDK prevents using authenticated endpoints before authentication (compile-time enforcement)

### Logging
- **`tracing` crate** -- structured, hierarchical logging with spans
- **Prometheus metrics** -- win rate, drawdown, latency, Sharpe ratio
- **Audit streams** -- barter-rs emits `EngineAudit` events for event-sourcing

### Error Recovery
- **Circuit breakers** -- Daily Loss Limit, Max Drawdown Halt, Composite Risk Score
- **"No Amnesia" persistence** -- retain critical risk state (HWM, Daily Loss) across restarts
- **Panic mode** -- emergency liquidation during data outages
- **Heartbeats** -- Polymarket SDK auto-sends heartbeat messages; disconnects cancel all open orders
- **`backoff` crate** -- exponential retry for WebSocket reconnection
- **`anyhow` / `thiserror`** -- structured error handling throughout the ecosystem

## 5. Key Design Principles

1. **`rust_decimal` everywhere** -- Never use `f64` for money
2. **Async-first with tokio** -- `mpsc` channel for inter-component communication
3. **Trait-based extensibility** -- Define traits for Strategy, RiskManager, ExecutionClient
4. **Indexed state management** -- O(1) lookups via indexed data structures
5. **Separate hot path from cold path** -- Engine processes market data (hot), audit stream feeds monitoring (cold)
6. **Type-level safety** -- Rust's type system enforces correct API usage at compile time
