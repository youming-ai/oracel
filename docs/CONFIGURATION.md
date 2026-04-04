# Configuration Guide

## Overview

The Polymarket 5m Bot uses a TOML configuration file (`config.toml`) for all runtime settings. On startup, it attempts to load this file; if loading/parsing fails, it logs the error and falls back to defaults, then validates the resulting config before trading. If `config.toml` doesn't exist, a default one is auto-generated.

## Configuration File Location

- **Development**: `./config.toml` (repository root)
- **Production**: Same location, typically symlinked or mounted

## Full Configuration Reference

```toml
[trading]
mode = "paper"                      # "paper" or "live"
paper_starting_balance = 100.0      # paper mode starting balance

[market]
stale_threshold_ms = 30000          # max BTC price age before skipping (ms)
min_ttl_ms = 30000                  # min remaining market TTL to enter (ms)

[polyclob]
gamma_api_url = "https://gamma-api.polymarket.com"

[strategy]
extreme_threshold = 0.95            # trade when yes/no ≥95% or ≤5%
fair_value = 0.5
position_size_usdc = 1.0            # per-trade size (min $1 enforced)
min_entry_price = 0.05              # reject candidates below
max_entry_price = 0.15              # reject candidates above
min_ttl_for_entry_ms = 90000        # min market TTL to enter (ms)
btc_trend_window_s = 30             # BTC trend lookback (s). 0=off
btc_trend_min_pct = 0.05            # min BTC % change for trend signal
circuit_breaker_window = 50         # sliding-window trade count. 0=off
circuit_breaker_min_win_rate = 0.05 # min win rate to keep trading

[risk]
max_fak_retries = 3                 # FAK retries per market window
fak_backoff_ms = 1000               # backoff after FAK rejection (ms)
daily_loss_limit_usdc = 50.0        # daily loss cap. 0=off

[polling]
signal_interval_ms = 1000           # main tick interval (ms)
status_interval_ms = 10000          # status log interval (ms)
market_refresh_secs = 60            # market discovery refresh (s)
settlement_check_secs = 15          # settlement poll interval (s)

[price_source]
source = "binance"                  # "binance" or "binance_ws"
symbol = "BTCUSDT"
buffer_max = 1000                   # max price ticks retained
buffer_min_ticks = 60               # min ticks before trading starts

[execution]
slippage_tolerance = 0.01           # 1% slippage on top of mid-price

[timeouts]
gamma_http_secs = 10
ws_connect_secs = 10
ws_max_backoff_secs = 60            # doubles on each retry
clob_price_secs = 10
clob_auth_secs = 15
clob_order_secs = 15
rpc_connect_secs = 30
rpc_redeem_secs = 30
balance_query_secs = 10

[redeem]
max_retries = 10
delay_secs = 5                      # between redemption txns (s)
concurrency = 5                     # CLI scan parallelism

[misc]
trade_log_flush_secs = 30
shutdown_timeout_secs = 5           # graceful shutdown wait (s)
market_search_windows = 5           # future 5m windows to search
resolution_price_threshold = 0.999  # winning resolution threshold

[time_windows]
window1_start = 0
window1_end = 12
window2_start = 12
window2_end = 24
```

## Section-by-Section Guide

### Trading Configuration

```toml
[trading]
mode = "paper"
paper_starting_balance = 100.0
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `mode` | string | `"paper"` | Trading mode: `"paper"` for simulation, `"live"` for real trading |
| `paper_starting_balance` | float | `100.0` | Starting balance for paper mode (ignored in live mode) |

**Paper Mode**:
- Simulated trading with no real orders
- Uses local UUIDs as order IDs
- No private key required

**Live Mode**:
- Places real orders on Polymarket CLOB
- Requires `PRIVATE_KEY` environment variable
- Balance synced from on-chain USDC wallet
- Enables automatic CTF redemption

---

### Market Configuration

```toml
[market]
stale_threshold_ms = 30000
min_ttl_ms = 30000
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `stale_threshold_ms` | integer | `30000` | Max age of BTC price data before considered stale (ms) |
| `min_ttl_ms` | integer | `30000` | Minimum remaining time before market expiry to place a trade (ms) |

---

### Strategy Configuration

```toml
[strategy]
extreme_threshold = 0.95
fair_value = 0.5
position_size_usdc = 1.0
min_entry_price = 0.05
max_entry_price = 0.15
min_ttl_for_entry_ms = 90000
btc_trend_window_s = 30
btc_trend_min_pct = 0.05
circuit_breaker_window = 50
circuit_breaker_min_win_rate = 0.05
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `extreme_threshold` | float | `0.95` | Market bias threshold for extreme sentiment (0.0-1.0) |
| `fair_value` | float | `0.50` | Fair-value assumption for binary outcome (0.0-1.0) |
| `position_size_usdc` | float | `1.0` | Target size per trade in USDC; runtime enforces $1 minimum |
| `min_entry_price` | float | `0.05` | Lower bound for candidate entry quote |
| `max_entry_price` | float | `0.15` | Upper bound for candidate entry quote |
| `min_ttl_for_entry_ms` | integer | `90000` | Minimum TTL remaining to enter a trade (ms) |
| `btc_trend_window_s` | integer | `30` | BTC trend lookback window (seconds). 0 = disabled |
| `btc_trend_min_pct` | float | `0.05` | Minimum BTC price change (% as decimal) for trend signal |
| `circuit_breaker_window` | integer | `50` | Sliding-window trade count. 0 = disabled |
| `circuit_breaker_min_win_rate` | float | `0.05` | Minimum win rate to keep trading |

#### Position Size

The bot targets `position_size_usdc` per trade, but enforces Polymarket's minimum order:

```text
shares = floor(position_size_usdc / entry_price)
actual_cost = shares * entry_price

if actual_cost < 1.0:
    shares = ceil(1.0 / entry_price)
    actual_cost = shares * entry_price
```

**Zero-share guard**: Orders resulting in 0 shares are rejected to prevent phantom trades.

---

### Risk Configuration

```toml
[risk]
max_fak_retries = 3
fak_backoff_ms = 1000
daily_loss_limit_usdc = 50.0
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_fak_retries` | integer | `3` | Maximum FAK order rejections per market window |
| `fak_backoff_ms` | integer | `1000` | Backoff after FAK rejection (ms) |
| `daily_loss_limit_usdc` | float | `50.0` | Daily loss cap in USDC. 0 = disabled |

---

### Price Source Configuration

```toml
[price_source]
source = "binance"
symbol = "BTCUSDT"
buffer_max = 1000
buffer_min_ticks = 60
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `source` | enum | `"binance"` | Price feed: `"binance"` or `"binance_ws"` |
| `symbol` | string | `"BTCUSDT"` | Trading pair symbol |
| `buffer_max` | integer | `1000` | Maximum price ticks retained in buffer |
| `buffer_min_ticks` | integer | `60` | Minimum buffer ticks before trading starts |

---

### Time Windows Configuration

```toml
[time_windows]
window1_start = 0
window1_end = 12
window2_start = 12
window2_end = 24
```

Two monitoring windows for the trade log dashboard (UTC hours, 0-24). Each window is a half-open interval `[start, end)`. Supports wrap-around (e.g., start=22, end=6).

---

## Configuration Validation

The bot validates the effective configuration on startup. Validation failures terminate startup with descriptive errors.

### Validation Rules

| Field | Validation |
|-------|------------|
| `signal_interval_ms` | > 0 |
| `extreme_threshold` | 0 < value < 1, and > fair_value |
| `fair_value` | 0 < value < 1 |
| `position_size_usdc` | > 0 |
| `min_entry_price`, `max_entry_price` | 0 < min < max < 1 |
| `min_ttl_for_entry_ms` | > 0 |
| `symbol` | Binance format (uppercase, no dash) |

---

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `PRIVATE_KEY` | Live mode | Wallet private key for CLOB authentication |
| `ALCHEMY_KEY` | Optional | Alchemy API key for Polygon RPC |

Create a `.env` file in the repository root:

```bash
# .env
PRIVATE_KEY=0x...
ALCHEMY_KEY=...
```

---

## Runtime Configuration Updates

Configuration is loaded once at startup. To apply changes:

1. Stop the bot (Ctrl+C or SIGTERM)
2. Edit `config.toml`
3. Restart the bot

The bot does not support hot-reloading configuration.
