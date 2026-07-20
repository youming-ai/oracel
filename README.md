# Polymarket BTC 5m Bot

Automated contrarian trader for Polymarket BTC 5-minute UP/DOWN markets. Binance supplies
real-time BTC prices; Polymarket supplies the market, order book, execution, and resolution.

The bot has exactly two modes:

- **paper** — simulates fills and PnL without a private key
- **live** — submits real FAK orders and redeems winning CTF positions

See [Trading flow](docs/ARCHITECTURE.md) and [Strategy](docs/STRATEGY.md) for the authoritative
behavior. `config.toml` is the authoritative configuration example.

## Quick start

```bash
# Validate, build, and test
cargo build --locked
cargo test --locked

# Paper mode is the checked-in default
cargo run --release --bin polybot
```

The terminal dashboard starts automatically. Press `q` or `Esc` to stop. For a service or log-only
run:

```bash
POLYBOT_HEADLESS=1 cargo run --release --bin polybot
```

## Live mode

Set the mode in `config.toml`:

```toml
[trading]
mode = "live"
```

Create `.env` from `.env.example` and set `PRIVATE_KEY`. `ALCHEMY_KEY` is optional; without it the
bot uses a public Polygon RPC endpoint.

Before enabling live mode:

1. Run paper mode through at least one entry and settlement.
2. Verify the wallet address, USDC balance, approvals, and Polymarket access.
3. Keep `position_size_usdc` small.
4. Confirm `logs/live/` is writable and backed up.

## Runtime files

Mode-specific files prevent paper state from contaminating live state:

```text
logs/
├── paper/
│   ├── bot.log.YYYY-MM-DD
│   ├── trades.csv
│   ├── balance
│   └── pending_positions.json
└── live/
    └── ...
```

Pending positions and balances are written atomically and restored after restart.

## Data sources

| Source | Purpose |
| --- | --- |
| Binance WebSocket | BTCUSDT price buffer and momentum filter |
| Polymarket Gamma API | Active-market discovery and official resolution |
| Polymarket CLOB | Buy quotes and live FAK execution |
| Polygon RPC | Live USDC balance and CTF redemption |

## Tools

```bash
# Derive CLOB credentials without writing secrets to disk
cargo run --release --bin polybot-tools -- --derive-keys

# Redeem one resolved market
cargo run --release --bin polybot-tools -- --redeem <market-slug>

# Scan the last 24 hours for redeemable positions
cargo run --release --bin polybot-tools -- --redeem-all
```

## Development checks

```bash
cargo build --locked
cargo test --locked
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo audit
```

## Project layout

```text
src/
├── main.rs                 # startup, logging, TUI/headless mode
├── bot.rs                  # orchestration and one-second trading tick
├── config.rs               # paper/live configuration and validation
├── tasks.rs                # market refresh, status, settlement, redemption
├── trade_log.rs            # shared CSV writer
├── data/
│   ├── binance.rs          # BTC WebSocket
│   ├── market_discovery.rs # Gamma discovery and resolution
│   └── polymarket.rs       # CLOB and Polygon clients
├── pipeline/
│   ├── price_source.rs
│   ├── decider.rs
│   ├── executor.rs
│   └── settler.rs
└── tui/
```

## Environment variables

| Variable | Required | Purpose |
| --- | --- | --- |
| `PRIVATE_KEY` | Live only | CLOB signing and CTF redemption |
| `ALCHEMY_KEY` | No | Reliable Polygon RPC |
| `RUST_LOG` | No | Log level, for example `debug` |
| `POLYBOT_HEADLESS` | No | Disable terminal UI when set |

Never commit `.env`, private keys, API credentials, or runtime logs.
