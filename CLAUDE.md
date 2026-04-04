# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Rust trading bot for Polymarket BTC 5-minute up/down binary markets. Monitors live BTC prices via WebSocket, fetches market quotes from Polymarket CLOB, and bets against extreme market sentiment. Includes a React web dashboard for monitoring.

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

# Dashboard (from dashboard/ directory)
bun install                    # install deps
bun run dev                    # dev server
BOT_MODE=live bun run dev      # dev server reading live logs
bun run build                  # production build (tsc -b && vite build)
bun run lint                   # eslint
```

## Architecture

### 4-Stage Trading Pipeline

The bot runs a linear pipeline each tick (`bot.rs:tick()`):

1. **Price Source** (`pipeline/price_source.rs`) — WebSocket price buffer from Binance
2. **Decider** (`pipeline/decider.rs`) — Detects extreme market sentiment → determines direction (Up/Down), evaluates entry price range, TTL, balance, daily loss limit → outputs `Trade` or `Pass`
3. **Executor** (`pipeline/executor.rs`) — Places FAK (Fill-And-Kill) limit orders, paper-simulated or live via CLOB
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

### Concurrency Model

- `tokio` async runtime with multiple spawned background tasks
- `Arc<RwLock<T>>` for shared mutable state (market state, account state, settler)
- `broadcast` channels for price distribution from WebSocket to pipeline
- `AtomicBool` for graceful shutdown coordination across tasks

### Dashboard

React + Vite + Tailwind app in `dashboard/`. Reads `trades.csv` and `balance` files from `logs/{mode}/` via a custom Vite middleware plugin (see `vite.config.ts`). BOT_MODE env var selects paper/live logs.

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

- `config.toml` — Main config. Validated on startup. Auto-generated with defaults if missing.
- `.env` — `PRIVATE_KEY` (live mode only), `ALCHEMY_KEY` (optional Polygon RPC)
- Two modes: `paper` (simulated, default $100 balance) and `live` (real orders, on-chain balance sync)
- `time_windows` section configures two monitoring windows for the dashboard (UTC hours, supports wrap-around)

## Logs & State Files

```
logs/{paper,live}/
  bot.log             # rolling daily log
  trades.csv          # trade entries & settlements
  balance             # current balance snapshot (atomic writes)
  time_windows.json   # dashboard monitoring windows config
```
