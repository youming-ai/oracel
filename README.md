# Polymarket 5m Bot

An automated trading bot for Polymarket BTC 5-minute up/down markets. It monitors live BTC prices via WebSocket (Binance or Coinbase), fetches market quotes from the Polymarket CLOB, and bets against extreme market sentiment. Supports both paper trading (simulated) and live trading with on-chain order placement and CTF redemption.

## Strategy Overview

- Buy `DOWN` when the market becomes extremely bullish (≥95%)
- Buy `UP` when the market becomes extremely bearish (≤5%)
- Fair value assumption: `0.50` for a 5-minute binary outcome
- Only trade when edge and momentum filter pass
- Position size: 1% of balance per trade, $1 minimum
- Risk warnings are logged for cooldown, loss streaks, and daily loss; only zero balance blocks trading

See [docs/STRATEGY.md](docs/STRATEGY.md) for the full strategy logic, decision flow, and risk controls.

## Documentation

Comprehensive documentation is available in the `docs/` directory:

- **[docs/STRATEGY.md](docs/STRATEGY.md)** - Trading strategy and decision flow
- **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** - System architecture and data flow
- **[docs/MODULES.md](docs/MODULES.md)** - Detailed module documentation
- **[docs/API.md](docs/API.md)** - API reference and data structures
- **[docs/CONFIGURATION.md](docs/CONFIGURATION.md)** - Configuration guide
- **[docs/DEPLOYMENT.md](docs/DEPLOYMENT.md)** - Deployment and operations guide

## Architecture

```text
Binance/Coinbase WS ──────► BTC price buffer (1s ticks)
                                   │
Polymarket CLOB REST ─────► Yes/No mid prices
                                   │
                         ┌─────────┴─────────┐
                         │     Pipeline       │
                         │  1. PriceSource    │  BTC price history (multi-exchange)
                         │  2. Signal         │  Extreme market detection
                         │  3. Decider        │  Edge, momentum checks, zero-balance guard
                         │  4. Executor       │  Paper UUID / Live FOK order
                         │  5. Settler        │  Expiry settlement + PnL
                         └─────────┬─────────┘
                                   │
                   ┌───────────────┼───────────────┐
                   │ Paper                         │ Live
                   │ Gamma API ──► market resolution │ Gamma API ──► market resolution
                   │                               │ CTF Redeemer ──► on-chain redeem
                   └───────────────────────────────┘
```

### Background Tasks

The main loop runs four concurrent tasks:

| Task | Interval | Purpose |
| --- | --- | --- |
| Signal tick | 1s | Fetch prices, evaluate signal, decide and execute |
| Settlement checker | 15s | Settle expired positions via Gamma API resolution |
| Market refresher | 60s | Discover the current active 5-minute market via Gamma API |
| Status printer | 10s | Log runtime summary (balance, PnL, streak, pending, TTL) |

## Repository Layout

```text
src/
├── main.rs                  # Main loop, bot state, CLI, persistence
├── config.rs                # Config definitions, defaults, validation
├── data/
│   ├── binance.rs           # Binance WebSocket client (NEW)
│   ├── chainlink.rs         # Polygon RPC URL selection (Alchemy fallback)
│   ├── coinbase.rs          # Coinbase Advanced Trade WebSocket client
│   ├── market_discovery.rs  # Gamma API market discovery and resolution
│   └── polymarket.rs        # CLOB client, order placement, CTF redemption
└── pipeline/
    ├── mod.rs               # Pipeline module
    ├── price_source.rs      # BTC price buffer with history (multi-exchange)
    ├── signal.rs            # Extreme market detection
    ├── decider.rs           # Trade decision, momentum, account state (risk logged)
    ├── executor.rs          # Paper/live order execution
    └── settler.rs           # Position settlement and PnL calculation

scripts/
└── watch.sh                 # Real-time terminal log monitor

deploy/
└── polybot.service          # systemd service template

logs/                        # Generated at runtime (gitignored)
├── paper/                   # Paper mode data
│   ├── bot.log              # Runtime log
│   ├── trades.csv           # Trade entries and settlements
│   ├── balance              # Current balance snapshot
│   └── state.json           # Persisted bot state
└── live/                    # Live mode data
    ├── bot.log
    ├── trades.csv
    ├── balance
    └── state.json
```

## Quick Start

```bash
# 1. Build
cargo build --release

# 2. Create config
cp config.example.json config.json

# 3. Run in paper mode (default)
cargo run --release

# 4. Monitor logs
scripts/watch.sh          # paper mode (default)
scripts/watch.sh live     # live mode
```

## CLI

```bash
# Run the bot (mode determined by config.json)
cargo run --release

# Derive Polymarket CLOB API credentials from PRIVATE_KEY
# Prints to terminal only, does not persist to disk
cargo run --release -- --derive-keys

# Scan the last 24 hours of markets and redeem winning positions on-chain
cargo run --release -- --redeem-all

# Redeem a specific market by slug
cargo run --release -- --redeem btc-updown-5m-1773926700
```

## Runtime Modes

### Paper

- Default mode (`trading.mode = "paper"` in `config.json`)
- Does not require `PRIVATE_KEY`
- Generates a local UUID as the order ID instead of placing a real order
- Settlement uses Gamma API market resolution
- Starts with $1,000 simulated balance (or restores from `logs/paper/balance`)

### Live

- Set `trading.mode` to `"live"` in `config.json`
- Requires `PRIVATE_KEY` in `.env`
- Authenticates with the Polymarket CLOB and places real FOK limit orders
- Balance is synced from the on-chain USDC wallet every tick
- Settlement uses Gamma API market resolution
- Enables CTF redeemer for automatic on-chain redemption of winning positions
- Uses Alchemy RPC when `ALCHEMY_KEY` is set, otherwise falls back to public Polygon RPC

## Environment Variables

The program reads `.env` from the repository root at startup.

| Variable | Required | Description |
| --- | --- | --- |
| `PRIVATE_KEY` | Live mode | Wallet private key for CLOB authentication and CTF redemption |
| `ALCHEMY_KEY` | Optional | Alchemy API key for Polygon RPC; improves reliability for Chainlink queries and on-chain operations |

## Configuration

Trading mode and all strategy parameters are configured in `config.json`. See `config.example.json` for a sample and `src/config.rs` for code defaults.

| Field | Default | Description |
| --- | --- | --- |
| `trading.mode` | `"paper"` | Runtime mode: `"paper"` or `"live"` |
| `market.stale_threshold_ms` | `30000` | Max age of BTC price data before considered stale (ms) |
| `market.min_ttl_ms` | `30000` | Minimum remaining time before market expiry to place a trade (ms) |
| `polyclob.gamma_api_url` | `https://gamma-api.polymarket.com` | Gamma API base URL |
| `price_source.source` | `"binance"` | Price feed: `"binance"`, `"binance_ws"`, `"coinbase"`, `"coinbase_ws"` |
| `price_source.symbol` | `"BTCUSDT"` | Trading pair symbol (e.g., "BTCUSDT" for Binance, "BTC-USD" for Coinbase) |
| `strategy.extreme_threshold` | `0.95` | Market bias threshold to consider sentiment extreme |
| `strategy.fair_value` | `0.50` | Fair-value assumption for a binary 5-minute outcome |
| `strategy.position_size_usdc` | `1.0` | Fixed position size per trade in USDC |
| `edge.edge_threshold_early` | `0.15` | Minimum edge required to place a trade (15%) |
| `risk.max_fok_retries` | `3` | Maximum retries for Fill-or-Kill orders |
| `polling.signal_interval_ms` | `1000` | Main signal loop interval in milliseconds |
| `polling.status_interval_ms` | `10000` | Status log printing interval in milliseconds |

### Price Source Configuration

The bot supports multiple price sources via the `price_source` config section:

```json
{
  "price_source": {
    "source": "binance",
    "symbol": "BTCUSDT"
  }
}
```

Available sources:
- `binance` (default): Binance WebSocket stream
- `binance_ws`: Binance WebSocket (explicit)
- `coinbase`: Coinbase WebSocket
- `coinbase_ws`: Coinbase WebSocket (explicit)

Symbol formats:
- Binance: `BTCUSDT`, `ETHUSDT` (no dash, uppercase)
- Coinbase: `BTC-USD`, `ETH-USD` (with dash)

The bot validates symbol format on startup and rejects mismatched configurations (e.g., using `BTCUSDT` with Coinbase source).

## Data Sources

| Source | Protocol | Purpose |
| --- | --- | --- |
| Binance | WebSocket | Live BTC/USDT price stream (default, low latency) |
| Coinbase Advanced Trade | WebSocket | Live BTC/USD price stream (alternative) |
| Polymarket CLOB | REST | Yes/No mid prices and live order placement |
| Gamma API | REST | Market discovery, slug lookup, resolution checks |
| CTF Contract | Polygon RPC | On-chain position balance queries and redemption |

## Logs and Monitoring

All logs are written to `logs/<mode>/` where mode is `paper` or `live`.

| File | Content |
| --- | --- |
| `bot.log` | Full runtime log with `[INIT]`, `[MKT]`, `[IDLE]`, `[SKIP]`, `[TRADE]`, `[SETTLED]`, `[STATUS]` prefixes |
| `trades.csv` | One row per trade entry and one row per settlement |
| `balance` | Current balance as a plain decimal (atomically updated) |
| `state.json` | Pending positions, streak counters, daily PnL |

Log tag reference:

| Tag | Meaning |
| --- | --- |
| `[IDLE]` | Pre-signal filter rejected (buffer filling, not extreme, TTL too short) |
| `[SKIP]` | Decider rejected (already traded, against trend, edge too low) |
| `[TRADE]` | Order placed (direction, price, edge, BTC price) |
| `[SETTLED]` | Position settled (WIN/LOSS, PnL, running W/L count) |
| `[STATUS]` | Periodic summary (mode, BTC, balance, PnL, streak, pending, TTL) |
| `[RISK]` | Risk warning triggered (cooldown, loss streak, daily loss); zero balance still blocks |

Terminal monitoring:

```bash
scripts/watch.sh          # paper mode (default)
scripts/watch.sh live     # live mode
```

## Deployment

The repository includes `deploy/polybot.service`, a systemd service template.

```bash
# Edit paths in polybot.service to match your install location, then:
sudo cp deploy/polybot.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now polybot
```

The bot handles `SIGINT` and `SIGTERM` for graceful shutdown: it persists state and balance to disk before exiting.

## Safety Features

- **Secret handling**: `PRIVATE_KEY` wrapped in `SecretString`; `--derive-keys` masks secret output
- **Decimal precision**: All financial calculations use `rust_decimal::Decimal`, never `f64`
- **Network resilience**: All HTTP/RPC calls have explicit timeouts (10–30s); WebSocket reconnects with exponential backoff
- **Graceful shutdown**: `SIGINT`/`SIGTERM` flush balance and state to disk
- **Config validation**: Bounds-checked on startup; invalid configs are rejected immediately
- **Atomic file writes**: Balance and state files use write-to-temp + rename to prevent corruption
- **Zero-share guard**: Orders with computed 0 shares are rejected to prevent phantom trades
- **Risk logging**: Cooldown, loss-streak, and daily-loss conditions are logged; zero-balance trades are still rejected
- **CI**: GitHub Actions pipeline with build, clippy, rustfmt, and `cargo audit`

## Recent Changes

### Multi-Exchange Price Sources
- Added Binance WebSocket support (default)
- Configurable via `price_source.source` and `price_source.symbol`
- Enum-based dispatch for performance (no trait objects)

### Risk Controls
- Cooldown and daily-loss conditions are logged as warnings but do not block trading
- Zero-balance trades are always rejected regardless of configuration

### WebSocket Improvements
- Simplified WebSocket task architecture: one client task + one consumer task per exchange
- Uses exchange timestamps (Binance `E` field) for more accurate price staleness detection
- Invalid symbol errors (`-1121`) now cause permanent failure instead of infinite reconnection loops
- Out-of-order tick protection: ignores timestamps that move backward

### Bug Fixes
- Fixed zero-share order bug: orders resulting in 0 shares are now rejected
- Fixed order ID slicing panic: safe handling of short order IDs
- Improved position combining safety: explicit error handling instead of unwrap
