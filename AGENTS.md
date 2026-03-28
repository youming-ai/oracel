# AGENTS.md

Coding agent instructions for the Polymarket 5m Bot repository.

## Project Overview

Rust trading bot for Polymarket BTC 5-minute up/down markets. Monitors live BTC prices via WebSocket, fetches market quotes from Polymarket CLOB, and bets against extreme market sentiment.

## Build/Lint/Test Commands

```bash
# Build (debug)
cargo build

# Build (release)
cargo build --release

# Run all tests
cargo test

# Run a single test (by name pattern)
cargo test test_trade_when_extreme_bullish
cargo test --test <test_name>

# Run tests for a specific module
cargo test --lib pipeline::decider

# Lint with clippy (must pass with no warnings)
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Format check
cargo fmt --all -- --check

# Format code
cargo fmt

# Security audit
cargo audit

# Full CI check (run before committing)
cargo build --locked && cargo test --locked && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo fmt --all -- --check
```

## Code Style Guidelines

### Formatting

- **Edition**: Rust 2021
- **Max line width**: 100 characters
- **Indentation**: 4 spaces (no tabs)
- **Newline style**: Unix (LF)
- Run `cargo fmt` before committing

### Imports

```rust
// Order: std → external crates → local modules (separated by blank lines)
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use rust_decimal::Decimal;
use tokio::sync::RwLock;

use crate::config::Config;
use crate::pipeline::signal::Direction;
```

### Types and Precision

- **Financial calculations**: Always use `rust_decimal::Decimal`, never `f64`
- **Decimal construction**: Use `Decimal::from_str_exact()` or helper function:

```rust
fn decimal(value: &str) -> Decimal {
    Decimal::from_str_exact(value).expect("valid decimal literal")
}
```

- **Secrets**: Wrap in `secrecy::SecretString`, expose only when needed:

```rust
use secrecy::{ExposeSecret, SecretString};
// Access: config.private_key.expose_secret()
```

### Naming Conventions

- **Modules**: `snake_case` (e.g., `market_discovery.rs`)
- **Types/Structs/Enums**: `PascalCase` (e.g., `DeciderConfig`, `TradingMode`)
- **Functions/Methods**: `snake_case` (e.g., `is_paper()`, `record_settlement()`)
- **Constants**: `SCREAMING_SNAKE_CASE` (e.g., `PRICE_BUFFER_MAX`, `MAX_BACKOFF_SECS`)
- **Struct fields**: `snake_case`
- **Enum variants**: `PascalCase`

### Visibility

- Prefer `pub(crate)` for internal APIs
- Only use `pub` for truly public interfaces
- Tests use `#[cfg(test)]` module pattern

### Error Handling

- Use `anyhow::Result` for fallible operations
- Use `.context()` to add context to errors:

```rust
use anyhow::Context;

let value = parse_config().context("failed to parse config")?;
```

- Use `anyhow::bail!` for early returns with errors:

```rust
anyhow::bail!("CLOB auth failed: {}", e);
```

- For WebSocket errors, distinguish permanent vs transient:

```rust
enum WsLoopError {
    Permanent(String),
    Transient(anyhow::Error),
}
```

### Logging

- Use `tracing` crate, not `println!` or `log`:

```rust
tracing::info!("[INIT] Starting balance: ${:.2}", balance);
tracing::warn!("[WS] Binance WS disconnected, reconnecting...");
tracing::error!("[EXEC] Order failed: {}", err);
```

- Use log prefixes in brackets: `[INIT]`, `[TRADE]`, `[SETTLED]`, `[STATUS]`, `[RISK]`, `[IDLE]`, `[SKIP]`

### Async Patterns

- Use `tokio` for async runtime
- Use `Arc<RwLock<T>>` for shared mutable state
- Use `broadcast` channels for one-to-many communication
- Use `tokio::time::timeout` for network operations with explicit timeouts (10-30s)

### Testing

- Unit tests in `#[cfg(test)]` module at bottom of file
- Use descriptive test names: `test_<what>_<condition>_<expected>`
- Use helper function for decimal creation in tests:

```rust
#[cfg(test)]
fn d(value: &str) -> Decimal {
    Decimal::from_str_exact(value).expect("valid decimal")
}
```

- For async tests: `#[tokio::test]`
- Test helpers in `src/pipeline/test_helpers.rs`

### Documentation

- Module-level doc comments: `//! Description`
- Public items should have doc comments: `/// Description`

## Project Structure

```
src/
├── main.rs              # Entry point, tracing setup, CLI
├── bot.rs               # Bot struct, main loop, order logic, trade recording
├── config.rs            # Configuration definitions and validation
├── state.rs             # BotState (in-memory: idle reasons, FAK state)
├── tasks.rs             # Background tasks: settlement, market refresh, status, balance write
├── lib.rs               # Library re-exports
├── cli.rs               # polybot-tools binary (derive-keys, redeem)
├── data/                # External data sources
│   ├── mod.rs           # Data module exports
│   ├── binance.rs       # Binance WebSocket client
│   ├── coinbase.rs      # Coinbase WebSocket client
│   ├── market_discovery.rs  # Gamma API market discovery
│   └── polymarket.rs    # CLOB client, order placement, CTF redemption, RPC URL selection
└── pipeline/            # Trading pipeline stages
    ├── mod.rs           # Module exports
    ├── price_source.rs  # Stage 1: BTC price buffer
    ├── signal.rs        # Stage 2: Extreme market detection
    ├── decider.rs       # Stage 3: Trade decision logic
    ├── executor.rs      # Stage 4: Order execution
    ├── settler.rs       # Stage 5: Position settlement
    └── test_helpers.rs  # Test utilities

dashboard/               # Real-time web dashboard (Bun + React)
```

## Configuration

- Config file: `config.json` (copy from `config.example.json`)
- Environment: `.env` file (see `.env.example`)
- All config validated on startup; invalid configs rejected immediately

## Safety Requirements

- **Never commit secrets** to the repository
- **Never use `f64` for financial calculations** - use `Decimal`
- **Always handle network timeouts** - use explicit timeouts (10-30s)
- **Atomic file writes** - write to temp file, then rename
- **Graceful shutdown** - handle `SIGINT`/`SIGTERM` and persist state

## Before Committing

1. Run `cargo fmt`
2. Run `cargo clippy --workspace --all-targets --all-features -- -D warnings`
3. Run `cargo test`
4. Ensure no secrets in code or commits
