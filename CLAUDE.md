# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Rust trading bot for Polymarket BTC 5-minute up/down binary markets. Monitors live BTC prices via WebSocket, fetches market quotes from Polymarket CLOB, and bets against extreme market sentiment. Includes a ratatui terminal dashboard.

Two binaries: `polybot` (main bot) and `polybot-tools` (CLI utilities for key derivation and position redemption).

## Build & Development Commands

```bash
# Build
cargo build                    # debug
cargo build --release          # release (opt-level=3, lto=thin)

# Test
cargo test                     # all tests
cargo test test_trade_when_extreme_bullish   # single test by name
cargo test --lib pipeline::decider           # tests in a module

# Lint & Format
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check     # check formatting
cargo fmt                      # apply formatting

# Full CI check (run before committing)
cargo build --locked && cargo test --locked && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo fmt --all -- --check

# Security audit
cargo audit
```

## Architecture

### 4-Stage Trading Pipeline

The bot runs a linear pipeline each tick (`bot.rs:tick()`):

1. **Price Source** (`pipeline/price_source.rs`) — WebSocket price buffer from Binance
2. **Decider** (`pipeline/decider.rs`) — Signal detection + trade decision: evaluates market extremeness, spread check, BTC trend, entry price range, TTL, balance, daily loss limit, circuit breaker → outputs `Trade` or `Pass`
3. **Executor** (`pipeline/executor.rs`) — Places FAK (Fill-And-Kill) limit orders via CLOB
4. **Settler** (`pipeline/settler.rs`) — Tracks pending positions, resolves outcomes via Gamma API

### Core Components

- `bot.rs` — Bot struct, main event loop (`run()`), per-tick logic (`tick()`), market refresh
- `state.rs` — In-memory bot/market state
- `tasks.rs` — Background async tasks: settlement checking (15s), market refresh (60s), status printing, balance persistence
- `config.rs` — Typed config with validation, loaded from `config.toml`
- `data/` — External data clients:
  - `binance.rs` — WebSocket price feed with auto-reconnect and exponential backoff
  - `market_discovery.rs` — Gamma API market slug generation (`btc-updown-5m-{timestamp}`) and resolution inference
  - `polymarket.rs` — CLOB client (unauthenticated price fetching + authenticated order placement), on-chain balance checker, CTF position redeemer
- `tui/` — ratatui terminal dashboard auto-launched with bot

### Concurrency Model

- `tokio` async runtime with multiple spawned background tasks
- `Arc<RwLock<T>>` for shared mutable state (market state, account state, settler, TUI state)
- `broadcast` channels for price distribution from WebSocket to pipeline
- `AtomicBool` for graceful shutdown coordination across tasks

### Terminal Dashboard (TUI)

ratatui dashboard automatically launched with the bot. Shows:

- Live BTC price and market info with TTL countdown
- Balance, PnL, win/loss stats, streak counter
- Recent trades table (scrolling via ↑↓)
- Current decision status

Keybindings: `q` or `Esc` to quit, `↑`/`↓` to scroll trade history.

## Key Conventions

- **Financial math**: Always `rust_decimal::Decimal`, never `f64`
- **Secrets**: `secrecy::SecretString` with `.expose_secret()` access
- **Error handling**: `anyhow::Result` with `.context()` for all fallible operations; `WsLoopError::Permanent` vs `Transient` for WebSocket errors
- **Logging**: `tracing` crate with bracket prefixes: `[INIT]`, `[MKT]`, `[TRADE]`, `[SETTLED]`, `[STATUS]`, `[RISK]`, `[IDLE]`, `[SKIP]`, `[WS]`
- **Imports**: std → external crates → local modules, separated by blank lines
- **Visibility**: Prefer `pub(crate)` over `pub` for internal APIs
- **File writes**: Atomic (write to temp file, then rename) to prevent corruption
- **Formatting**: 100-char max width, 4 spaces, Unix newlines (`rustfmt.toml`)
- **Tests**: `#[cfg(test)]` module at bottom of file; helper `d()` for Decimal creation in tests; async tests use `#[tokio::test]`

## Configuration

- `config.toml` — Main config. Validated on startup.
- `.env` — `PRIVATE_KEY` (required), `ALCHEMY_KEY` (optional Polygon RPC)
- Always live mode — no paper simulation

## Logs & State Files

```
logs/
  bot.log       # rolling daily log
  trades.csv    # trade entries & settlements
  balance       # current balance snapshot (atomic writes)
```
