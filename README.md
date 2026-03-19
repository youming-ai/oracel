# Polymarket 5m Bot

An automated trading bot for Polymarket BTC 5-minute up/down markets. It uses Coinbase for live BTC prices, Polymarket for 5-minute market quotes, bets against extreme market sentiment, and supports on-chain redemption in live mode.

## Strategy Overview

- Buy `DOWN` when the market becomes extremely bullish
- Buy `UP` when the market becomes extremely bearish
- Use `0.50` as the approximate fair value for a 5-minute window
- Only place a trade when edge, risk checks, and the momentum filter all pass

See `STRATEGY.md` for the full strategy logic.

## Architecture

```text
Coinbase WS -> Live BTC price (signal input)
                |
Polymarket REST -> Yes/No prices
                |
          +-------------------------+
          | Pipeline                |
          | 1. PriceSource          |
          | 2. Signal               |
          | 3. Decider              |
          | 4. Executor             |
          | 5. Settler              |
          +-------------------------+
                |
Paper: Chainlink Oracle -> BTC settlement price (Polygon)
Live: Gamma API -> real market resolution status
                |
CTF Redeemer -> On-chain redemption in live mode
```

## Repository Layout

```text
src/
|- main.rs                  # Main loop, CLI, and logging setup
|- config.rs                # Config definitions and defaults
|- data/
|  |- chainlink.rs          # Chainlink BTC/USD oracle RPC access
|  |- coinbase.rs           # Coinbase Advanced Trade WebSocket client
|  |- market_discovery.rs   # Gamma API market discovery
|  `- polymarket.rs         # CLOB client and CTF redeem logic
`- pipeline/
   |- price_source.rs       # BTC price buffer
   |- signal.rs             # Market signal calculation
   |- decider.rs            # Trade decision and risk control
   |- executor.rs           # Paper/live order execution
   `- settler.rs            # Expiry settlement and trade logging

scripts/
`- watch.sh                 # Real-time terminal monitor

deploy/
`- polybot.service          # systemd service file

logs/                       # Generated at runtime
|- paper/                   # Paper mode data
|  |- bot.log
|  |- trades.csv
|  |- balance
|  `- state.json
`- live/                    # Live mode data
   |- bot.log
   |- trades.csv
   |- balance
   `- state.json
```

## Quick Start

```bash
# 1. Build
cargo build --release

# 2. Create config
cp config.example.json config.json

# 3. Run in paper mode
cargo run --release

# 4. Monitor logs
scripts/watch.sh          # paper mode (default)
scripts/watch.sh live     # live mode
```

## CLI

```bash
# Run the bot normally
cargo run --release

# Derive Polymarket CLOB API credentials
# Prints to the terminal and does not write back to .env
cargo run --release -- --derive-keys

# Scan the last 24 hours of markets and try on-chain redemption
cargo run --release -- --redeem-all
```

## Runtime Modes

### Paper

- Default mode; `TRADING_MODE` in `.env` defaults to `paper`
- Does not require `PRIVATE_KEY`
- Uses a locally generated order ID instead of placing a real order
- Uses local settlement simulation: Chainlink BTC/USD when available, and the latest Coinbase price if Chainlink fails

### Live

- Requires `PRIVATE_KEY` in `.env`
- Creates an authenticated Polymarket client and places real orders
- Enables the CTF redeemer for on-chain redemption of redeemable positions
- Prefers `ALCHEMY_KEY` for Polygon RPC and falls back to the public Polygon RPC otherwise
- Uses Gamma market resolution data to decide local win/loss accounting instead of BTC-price simulation

## Environment Variables

The program reads `.env` from the repository root at startup.

| Variable | Required | Description |
| --- | --- | --- |
| `PRIVATE_KEY` | Required in live mode | Wallet private key used for CLOB auth and CTF redeem |
| `ALCHEMY_KEY` | Optional | Polygon RPC key for faster Chainlink queries and redeem calls in live mode |
| `TRADING_MODE` | Optional | Runtime mode; set to `paper` or `live`. Overrides `trading.mode` in `config.json` if both are present |

`--derive-keys` derives `POLY_API_KEY`, `POLY_API_SECRET`, and `POLY_PASSPHRASE` from `PRIVATE_KEY`, but only prints them to the terminal and does not write them back to `.env`.

## Configuration

The market series (`btc-updown-5m`) is hardcoded. See `config.example.json` for a sample config and `src/config.rs` for the full code defaults.

| Field | Default | Description |
| --- | --- | --- |
| `market.window_minutes` | `5.0` | Market window length |
| `polyclob.gamma_api_url` | `https://gamma-api.polymarket.com` | Gamma API base URL |
| `strategy.extreme_threshold` | `0.80` | Extreme sentiment threshold |
| `strategy.fair_value` | `0.50` | Fair-value assumption |
| `strategy.btc_tiebreaker_usd` | `5.0` | Settlement tie-break threshold when BTC price change is very small |
| `strategy.momentum_threshold` | `0.001` | Momentum filter threshold (0.1%) |
| `strategy.momentum_lookback_ms` | `120000` | Momentum lookback window (2 minutes) |
| `edge.edge_threshold_early` | `0.15` | Minimum edge required to place a trade |
| `risk.max_consecutive_losses` | `8` | Circuit-breaker threshold for consecutive losses |
| `risk.max_daily_loss_pct` | `0.10` | Daily loss percentage limit |
| `risk.cooldown_ms` | `5000` | Cooldown between trades |
| `polling.signal_interval_ms` | `1000` | Main signal loop interval |

Note: the checked-in `config.json` is a current sample runtime config, not necessarily the same as the code defaults. `TRADING_MODE` in `.env` overrides `trading.mode` in `config.json`.

## Data Sources

- Coinbase Advanced Trade WS: live BTC pricing
- Polymarket CLOB REST: Yes/No quotes and live order placement
- Gamma API: discovery of the current 5-minute market and live-mode resolution checks
- Chainlink BTC/USD Oracle on Polygon: paper-mode settlement pricing and redeem-related on-chain reads

## Logs and Monitoring

- `logs/<mode>/bot.log`: main runtime log (paper or live)
- `logs/<mode>/trades.csv`: appended on both trade entry and settlement
- `logs/<mode>/balance`: current balance snapshot
- `scripts/watch.sh [mode]`: terminal monitor (defaults to `paper`)
- periodic `[STATUS]` log line: built-in runtime summary printed every 10 seconds

## Deployment

The repository includes `deploy/polybot.service`, a systemd service template. Edit the `WorkingDirectory` and `ExecStart` paths to match your actual install location before enabling the service.

```bash
# Example setup
sudo cp deploy/polybot.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now polybot
```

The bot handles `SIGINT` and `SIGTERM` for graceful shutdown: it flushes the current balance to disk and exits cleanly.

## Recent Changes

- **Security**: `PRIVATE_KEY` is wrapped in `SecretString`; `--derive-keys` masks secret output
- **Precision**: All financial calculations use `rust_decimal::Decimal` instead of `f64`
- **Network resilience**: All network calls have explicit timeouts; WebSocket reconnects with exponential backoff
- **Graceful shutdown**: Handles `SIGINT`/`SIGTERM` with balance flush
- **Config validation**: Bounds-checked on startup; invalid configs are rejected immediately
- **CI**: GitHub Actions pipeline with build, clippy, rustfmt, and `cargo audit`
