# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Polymarket 5-minute BTC up/down binary options trading bot written in Rust. It ingests real-time BTC prices via WebSocket (Binance or Coinbase), detects extreme market sentiment on Polymarket, and places contrarian trades.

## Build & Run Commands

```bash
cargo build --release              # Build optimized binary
cargo run --release                # Run the bot (mode from config.json)
cargo run --release --bin polybot-tools -- --derive-keys   # Derive Polymarket CLOB API credentials
cargo run --release --bin polybot-tools -- --redeem-all    # Redeem winning positions (last 24h)
cargo test --locked                # Run all tests
cargo clippy --workspace --all-targets --all-features -- -D warnings  # Lint (CI-strict)
cargo fmt --all -- --check         # Format check
cargo audit                        # Security audit (requires cargo-audit)
```

## Formatting

- Edition 2021, max line width 100, 4-space indentation, Unix newlines (see `rustfmt.toml`)
- Clippy runs with `-D warnings` — all warnings are errors in CI

## Architecture

Pipeline architecture with 5 sequential stages, all driven by a tokio async event loop in `src/bot.rs` (entry point and tracing setup in `src/main.rs`, background tasks in `src/tasks.rs`, in-memory bot state in `src/state.rs`):

```
PriceSource → Signal → Decider → Executor → Settler
```

1. **PriceSource** (`src/pipeline/price_source.rs`): Ingests BTC prices from exchange WebSocket into a rolling buffer (max 1000 ticks). Lock-free read path via `Arc<RwLock<>>`.
2. **Signal** (`src/pipeline/signal.rs`): Computes market bias (`mkt_up = yes_price / (yes_price + no_price)`). Emits Up/Down when `extreme_threshold` is breached.
3. **Decider** (`src/pipeline/decider.rs`): Enforces one-trade-per-window, checks edge (`fair_value - cheap_side_price`), validates balance/staleness/TTL.
4. **Executor** (`src/pipeline/executor.rs`): Paper mode generates UUID order IDs; live mode places FAK orders via Polymarket CLOB SDK.
5. **Settler** (`src/pipeline/settler.rs`): Tracks pending positions, settles at expiry via Gamma API resolution, calculates PnL.

### Data layer (`src/data/`)

- `binance.rs` / `coinbase.rs`: Exchange WebSocket clients with auto-reconnect
- `polymarket.rs`: CLOB client for price queries, order placement, CTF balance/redemption, and RPC URL selection
- `market_discovery.rs`: Gamma API market discovery by slug pattern

### Main event loop (`src/bot.rs`)

Four concurrent `tokio::select!` tasks (spawned in `src/tasks.rs`): signal tick (1s), settlement check (15s), market refresh (60s), status print (10s). Balance is persisted atomically (tmp + rename) to `logs/{mode}/balance`. CLI tools (`--derive-keys`, `--redeem-all`, `--redeem`) live in a separate `polybot-tools` binary (`src/cli.rs`). In-memory bot state (idle reasons, FAK state) lives in `src/state.rs`.

## Key Conventions

- **All financial math uses `rust_decimal::Decimal`** — never use f64 for money/prices.
- **Secrets**: `PRIVATE_KEY` wrapped in `secrecy::SecretString`. Never log or serialize secrets.
- **Shared state**: `Arc<RwLock<T>>` pattern for cross-task data.
- **Error handling**: `anyhow::Result` throughout; HTTP/RPC calls have explicit timeouts (10-30s).
- **File persistence**: Always write to temp file then rename (atomic).
- **Config validation**: All config values bounds-checked at startup in `src/config.rs`.

## Configuration

- `config.json` (from `config.example.json`): Trading mode, thresholds, price source, polling intervals
- `.env` (from `.env.example`): `PRIVATE_KEY` (required for live), `ALCHEMY_KEY` (optional)

## Monitoring

```bash
# Web dashboard (from dashboard/ directory)
cd dashboard && bun run dev              # Paper mode
BOT_MODE=live bun run dev                # Live mode
```

Log files are in `logs/{paper,live}/` — `bot.log`, `trades.csv`, `balance`.
