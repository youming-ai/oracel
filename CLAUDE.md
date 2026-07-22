# CLAUDE.md

## Project

Rust Binance Prediction BTC five-minute UP/DOWN trading bot with a ratatui dashboard.

The bot uses only Binance services:

- Binance BTCUSDT WebSocket for spot price, momentum, and volatility.
- Binance Web3 Wallet Prediction REST API for market discovery, order books, quotes, orders,
  positions, settlement, and redemption.

Binaries:

- `binance-5m-bot` — main Paper/Live bot
- `binance-5m-tools` — API diagnostics and manual redemption recovery

## Commands

```bash
cargo build --locked
cargo test --locked
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo fmt
cargo audit

# Full local gate
cargo build --locked && cargo test --locked && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo fmt --all -- --check && cargo audit

# Verify Binance account/API access; never places an order
cargo run --release --bin binance-5m-tools -- --check

# Paper bot
BINANCE_5M_HEADLESS=1 cargo run --release --bin binance-5m-bot
```

## Architecture

### Pipeline

1. `pipeline/price_source.rs` — Binance BTCUSDT WebSocket rolling buffer
2. `pipeline/decider.rs` — deterministic probability/value/risk decision
3. `pipeline/executor.rs` — Paper fill or Binance Prediction MARKET/FOK order
4. `pipeline/settler.rs` — persisted accepted-order and open-position state

### Data client

`data/binance_prediction.rs` owns official Prediction REST API interaction:

- wallet selection and USDT payment balance;
- active BTC five-minute market discovery and strict UP/DOWN token mapping;
- order-book walks and executable quote prices;
- Get Quote / Place Order execution;
- order-history reconciliation;
- Paper reference/end-price resolution;
- live settled-position history and redemption.

### Concurrency

- `Arc<RwLock<T>>` protects market, account, tracker, and TUI state.
- Binance WebSocket and background refresh/settlement/status tasks share an `AtomicBool` shutdown
  flag.
- A live accepted order whose fill state is unknown is persisted and blocks all new entries until
  reconciled.

## Key conventions

- Financial values: `rust_decimal::Decimal`, never `f64`.
- Errors: `anyhow::Result` with context at external boundaries.
- Secrets: `secrecy::SecretString`; never log API credentials.
- Logging prefixes: `[INIT]`, `[MKT]`, `[BOOK]`, `[TRADE]`, `[EXEC]`, `[SETTLED]`, `[REDEEM]`,
  `[STATUS]`, `[RISK]`, `[IDLE]`, `[SKIP]`, `[WS]`.
- Use fixed-USDT amounts. Binance Prediction MARKET/FOK orders require roughly 1.5 USDT minimum;
  configuration enforces a 2 USDT minimum.
- Persist state atomically by writing a temporary file and renaming it.
- Prefer `pub(crate)` for internal APIs.
- Format: rustfmt, 100-character width, Unix newlines.

## Configuration

`config.toml` contains non-secret settings. `.env` contains:

```text
BINANCE_API_KEY
BINANCE_API_SECRET
BINANCE_PREDICTION_WALLET_ID      # optional unless multiple wallets
BINANCE_PREDICTION_WALLET_ADDRESS # optional unless multiple wallets
```

Paper and Live both require Binance API credentials because Binance Prediction endpoints are signed
by the current SDK. Live additionally requires a selected registered Prediction Wallet.

The checked-in mode is `paper`. Live requires both `mode = "live"` and
`allow_uncalibrated_model_live = true` until calibration requirements are met.

## Runtime files

```text
logs/binance/<mode>/
├── bot.log.YYYY-MM-DD
├── trades.csv
├── observations.csv
├── outcomes.csv
├── balance
└── pending_positions.json
```

`observations.csv` is the calibration dataset; `outcomes.csv` contains official labels for entered
markets. Do not infer success from win rate alone.
