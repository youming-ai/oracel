# Polymarket 5m Bot

An automated trading bot for Polymarket BTC 5-minute up/down markets. It monitors live BTC prices via Coinbase WebSocket, fetches market quotes from the Polymarket CLOB, and bets against extreme market sentiment. Supports both paper trading (simulated) and live trading with on-chain order placement and CTF redemption.

## Strategy Overview

- Buy `DOWN` when the market becomes extremely bullish (>80%)
- Buy `UP` when the market becomes extremely bearish (<20%)
- Fair value assumption: `0.50` for a 5-minute binary outcome
- Only trade when edge, risk checks, and momentum filter all pass
- Position size: 1% of balance per trade, $1 minimum

See [STRATEGY.md](STRATEGY.md) for the full strategy logic, decision flow, and risk controls.

## Architecture

```text
Coinbase WS ──────────► BTC price buffer (1s ticks)
                              │
Polymarket CLOB REST ──► Yes/No mid prices
                              │
                    ┌─────────┴─────────┐
                    │     Pipeline       │
                    │  1. PriceSource    │  BTC price history
                    │  2. Signal         │  Extreme market detection
                    │  3. Decider        │  Edge, risk, momentum checks
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
│   ├── chainlink.rs         # Polygon RPC URL selection (Alchemy fallback)
│   ├── coinbase.rs          # Coinbase Advanced Trade WebSocket client
│   ├── market_discovery.rs  # Gamma API market discovery and resolution
│   └── polymarket.rs        # CLOB client, order placement, CTF redemption
└── pipeline/
    ├── mod.rs               # Pipeline module
    ├── price_source.rs      # BTC price buffer with history
    ├── signal.rs            # Extreme market detection
    ├── decider.rs           # Trade decision, risk controls, account state
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
| `market.window_minutes` | `5.0` | Market window length in minutes |
| `polyclob.gamma_api_url` | `https://gamma-api.polymarket.com` | Gamma API base URL |
| `strategy.extreme_threshold` | `0.80` | Market bias threshold to consider sentiment extreme |
| `strategy.fair_value` | `0.50` | Fair-value assumption for a binary 5-minute outcome |
| `strategy.btc_tiebreaker_usd` | `5.0` | BTC price change threshold (unused after settlement refactor) |
| `strategy.momentum_threshold` | `0.001` | BTC momentum threshold (0.1%) to filter counter-trend trades |
| `strategy.momentum_lookback_ms` | `120000` | Momentum lookback window in milliseconds (2 minutes) |
| `edge.edge_threshold_early` | `0.15` | Minimum edge required to place a trade (15%) |
| `risk.max_consecutive_losses` | `8` | Circuit breaker: stop trading after N consecutive losses |
| `risk.max_daily_loss_pct` | `0.10` | Daily loss limit as fraction of balance (10%) |
| `risk.cooldown_ms` | `5000` | Minimum milliseconds between trades |
| `polling.signal_interval_ms` | `1000` | Main signal loop interval in milliseconds |

## Data Sources

| Source | Protocol | Purpose |
| --- | --- | --- |
| Coinbase Advanced Trade | WebSocket | Live BTC/USD price stream (1-second ticks) |
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
| `state.json` | Pending positions, streak counters, daily PnL, pause timer |

Log tag reference:

| Tag | Meaning |
| --- | --- |
| `[IDLE]` | Pre-signal filter rejected (buffer filling, not extreme, TTL too short) |
| `[SKIP]` | Decider rejected (already traded, against trend, edge too low) |
| `[TRADE]` | Order placed (direction, price, edge, BTC price) |
| `[SETTLED]` | Position settled (WIN/LOSS, PnL, running W/L count) |
| `[STATUS]` | Periodic summary (mode, BTC, balance, PnL, streak, pending, TTL) |

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
- **CI**: GitHub Actions pipeline with build, clippy, rustfmt, and `cargo audit`
