# Binance Prediction BTC 5m Bot

A Paper-first Binance Prediction trader for BTC five-minute UP/DOWN markets.

- **Binance Prediction API** discovers markets, reads order books, requests quotes, places orders,
  reconciles fills, reads settlement, and redeems winners.
- **Binance BTCUSDT WebSocket** provides the low-latency spot price, momentum, and realized
  volatility used by the model.
- No third-party prediction exchange, oracle feed, EVM RPC, CLOB, or private-key wallet integration
  is used.

The bot has two modes:

- **paper** — uses live Binance market data and books, simulates Binance fees/fills, and settles
  from the official Binance Prediction market end price.
- **live** — sends real Binance Prediction `MARKET/FOK` orders through the authenticated API and
  redeems winning tokens through the same API.

Read [Architecture](docs/ARCHITECTURE.md) and [Strategy](docs/STRATEGY.md) before running it.

## Prerequisites

1. A Binance account and API key allowed to access Binance Web3 Wallet Prediction endpoints.
2. `BINANCE_API_KEY` and `BINANCE_API_SECRET` in `.env`.
3. For live mode: a registered Binance Prediction Wallet and an enabled USDT Spot or Funding payment
   account.
4. Regional eligibility for Binance Prediction Trading.

Create `.env` from `.env.example`; never commit the real file.

```env
BINANCE_API_KEY=
BINANCE_API_SECRET=

# Optional only if more than one Prediction Wallet is registered
BINANCE_PREDICTION_WALLET_ID=
BINANCE_PREDICTION_WALLET_ADDRESS=
```

Validate access before running the bot:

```bash
cargo run --release --bin binance-5m-tools -- --check
```

This command reads wallets, the configured USDT payment balance, and the active BTC five-minute
market. It never places an order.

## Quick start

```bash
cargo build --locked
cargo test --locked

# The checked-in configuration is Paper mode.
cargo run --release --bin binance-5m-bot

# Service/log-only operation
BINANCE_5M_HEADLESS=1 cargo run --release --bin binance-5m-bot
```

Press `q` or `Esc` to stop the dashboard.

## Live mode

Live mode is deliberately guarded while the probability baseline remains uncalibrated:

```toml
[trading]
mode = "live"
allow_uncalibrated_model_live = true
```

Set both fields only after you have validated Binance API access and reviewed the Paper dataset.
The default live order is a `$2` USDT `MARKET/FOK` order because Binance Prediction market orders
require roughly `$1.50` minimum input.

Before live trading:

1. Run `binance-5m-tools --check` successfully.
2. Confirm the returned wallet is the intended Prediction Wallet.
3. Confirm `payment_account` (`spot` or `funding`) and `funding_source` (`cex` or `mpc`).
4. Collect and evaluate Paper observations and outcomes.
5. Keep `position_size_usdt` at the minimum safe size initially.

## Runtime files

Exchange-specific paths prevent old or unrelated state from being reused:

```text
logs/binance/
├── paper/
│   ├── bot.log.YYYY-MM-DD
│   ├── trades.csv
│   ├── observations.csv
│   ├── outcomes.csv
│   ├── balance
│   ├── account_state.json
│   ├── pending_positions.json
│   └── state_write_failed      # only after a failed state write
└── live/
    └── ...
```

`balance`, `account_state.json`, and `pending_positions.json` use write-to-temp plus rename.
`account_state.json` carries the risk counters (daily loss/trade caps, loss-streak cooldown,
circuit-breaker window) so a restart cannot silently reset them; a corrupt file fails startup.
Pending entries include accepted-but-not-yet-visible live orders so an uncertain submission
cannot be silently retried.

If a state write fails, the bot halts new entries (settlement continues) and writes a
`state_write_failed` marker; the halt is sticky and survives restart. Do not just delete the
marker: reconcile `account_state.json` and `pending_positions.json` against `trades.csv`,
`outcomes.csv`, and Binance history first, then remove the marker and restart. See
[Architecture](docs/ARCHITECTURE.md#state-write-failure-and-the-durability-halt).

## Data and execution sources

| Binance service | Purpose |
| --- | --- |
| BTCUSDT WebSocket | Current spot price, 15s/30s momentum, realized volatility |
| Prediction Market Detail | Five-minute reference/open price, official end price, outcome token mapping |
| Prediction Order Book | Executable bids, asks, weighted fill price, and depth |
| Prediction Get Quote / Place Order | Value-checked `MARKET/FOK` live execution |
| Prediction Position APIs | Fill reconciliation and official live PnL/settlement |
| Prediction Redeem API | Claim winning tokens |

## Tools

```bash
# Inspect API access, wallet, balance, and current BTC market
cargo run --release --bin binance-5m-tools -- --check

# Explicitly submit one redemption recovery request
cargo run --release --bin binance-5m-tools -- --redeem <prediction-token-id>
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
├── main.rs                       # startup, logging, dashboard/headless mode
├── bot.rs                        # one-second Binance trading loop
├── config.rs                     # Binance paper/live configuration and validation
├── tasks.rs                      # market refresh, reconciliation, settlement, redemption
├── trade_log.rs                  # accounting and model CSV writers
├── data/
│   ├── binance.rs                # BTCUSDT WebSocket
│   └── binance_prediction.rs     # official Prediction REST API
├── pipeline/
│   ├── price_source.rs           # rolling spot price and volatility buffer
│   ├── decider.rs                # deterministic probability/value gates
│   ├── executor.rs               # Paper or Binance MARKET/FOK execution
│   └── settler.rs                # persisted Binance order/position state
└── tui/
```

## Environment variables

| Variable | Required | Purpose |
| --- | --- | --- |
| `BINANCE_API_KEY` | Yes | Signed Binance Prediction API access |
| `BINANCE_API_SECRET` | Yes | Request signing secret |
| `BINANCE_PREDICTION_WALLET_ID` | Live/multiple wallets | Explicit Prediction Wallet selection |
| `BINANCE_PREDICTION_WALLET_ADDRESS` | Live/multiple wallets | Explicit Prediction Wallet selection |
| `RUST_LOG` | No | Log level, for example `debug` |
| `BINANCE_5M_HEADLESS` | No | Disable terminal dashboard when set |
