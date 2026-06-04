# Configuration Guide

## Overview

The Polymarket 5m Bot uses a TOML configuration file (`config.toml`) for all runtime settings. On startup, it attempts to load this file; if loading/parsing fails, it logs the error and falls back to defaults, then validates the resulting config before trading.

## Configuration File Location

- **Development**: `./config.toml` (repository root)
- **Production**: Same location, but typically symlinked or mounted

## Creating Configuration

Edit `config.toml` with your settings.

## Full Configuration Reference

```toml
{
  "market": {
    "stale_threshold_ms": 30000,
    "min_ttl_ms": 30000
  },
  "polyclob": {
    "gamma_api_url": "https://gamma-api.polymarket.com"
  },
  "price_source": {
    "symbol": "BTCUSDT"
  },
  "strategy": {
    "extreme_threshold": 0.95,
    "fair_value": 0.5,
    "position_size_usdc": 1.0,
    "min_entry_price": 0.02,
    "max_entry_price": 0.06,
    "min_ttl_for_entry_ms": 120000
  },
  "risk": {
    "max_fak_retries": 3,
    "fak_backoff_ms": 3000,
    "daily_loss_limit_usdc": 0.0
  },
  "polling": {
    "signal_interval_ms": 1000,
    "status_interval_ms": 10000
  },
  "execution": {
    "slippage_tolerance": 0.01
  }
}
```

## Section-by-Section Guide

### Market Configuration

```toml
"market": {
  "stale_threshold_ms": 30000,
  "min_ttl_ms": 30000
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `stale_threshold_ms` | integer | `30000` | Max age of BTC price data before considered stale (milliseconds) |
| `min_ttl_ms` | integer | `30000` | Minimum remaining time before market expiry to place a trade (milliseconds) |

The stale threshold ensures you don't trade on old price data. The minimum TTL prevents placing trades too close to settlement when prices may be volatile.

---

### Polymarket CLOB Configuration

```toml
"polyclob": {
  "gamma_api_url": "https://gamma-api.polymarket.com"
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `gamma_api_url` | string | `"https://gamma-api.polymarket.com"` | Gamma API base URL for market discovery and resolution |

**Note**: This is different from the CLOB API URL, which is configured internally.

---

### Price Source Configuration

```toml
"price_source": {
  "symbol": "BTCUSDT"
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `symbol` | string | `"BTCUSDT"` | Binance trading pair (e.g. `BTCUSDT`, `ETHUSDT`) |

**Symbol Format Validation**:
- The bot validates symbol format on startup against the Binance format
- Format: uppercase letters and digits only, no dashes (e.g. `BTCUSDT`, `ETHUSDT`, `SOLUSDT`)
- Invalid formats cause startup to fail with a descriptive error

---

### Strategy Configuration

```toml
"strategy": {
  "extreme_threshold": 0.95,
  "fair_value": 0.5,
  "position_size_usdc": 1.0,
  "min_entry_price": 0.02,
  "max_entry_price": 0.06,
  "min_ttl_for_entry_ms": 120000
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `extreme_threshold` | float | `0.95` | Market bias threshold to consider sentiment extreme (0.0-1.0) |
| `fair_value` | float | `0.50` | Fair-value assumption for binary outcome (0.0-1.0) |
| `position_size_usdc` | float | `1.0` | Configured target size per trade in USDC; runtime enforces a $1 minimum order |
| `min_entry_price` | float | `0.02` | Lower bound for candidate entry quote; candidates below this price are rejected |
| `max_entry_price` | float | `0.06` | Upper bound for candidate entry quote; candidates above this price are rejected |
| `min_ttl_for_entry_ms` | integer | `120000` | Strategy-level TTL floor; candidate must have at least this much time remaining to enter |

#### Extreme Threshold

Determines when market sentiment is considered extreme:

```
if market_bias > 0.95 → Extremely bullish → Buy DOWN
if market_bias < 0.05 → Extremely bearish → Buy UP
otherwise → No trade
```

**Examples**:
- `0.95` (default): Trade when market is ≥95% or ≤5%
- `0.90`: More aggressive, trade at >90% or <10%
- `0.97`: More conservative, trade at >97% or <3%

#### Position Size

The bot targets `position_size_usdc` per trade, but enforces Polymarket's minimum order:

```text
shares = floor(position_size_usdc / entry_price)
actual_cost = shares * entry_price

if actual_cost < 1.0:
    shares = ceil(1.0 / entry_price)
    actual_cost = shares * entry_price
```

Because shares are floored to whole numbers, `actual_cost` can be below configured
`position_size_usdc`; when the $1 minimum bump triggers, it can also exceed configured
`position_size_usdc`.

**Zero-share guard**: Orders resulting in 0 shares are rejected to prevent phantom trades.

---

### Risk Configuration

```toml
"risk": {
  "max_fak_retries": 3,
  "fak_backoff_ms": 3000,
  "daily_loss_limit_usdc": 0.0
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_fak_retries` | integer | `3` | Maximum FAK order rejections before giving up on market window |
| `fak_backoff_ms` | integer | `3000` | Milliseconds to wait after FAK rejection before retrying |
| `daily_loss_limit_usdc` | float | `0.0` | Daily loss cap in USDC; when daily PnL drops below `-daily_loss_limit_usdc`, new trades are blocked for the day (`0` disables the gate) |

The FAK retry mechanism handles temporary liquidity issues when placing orders on the CLOB. After a rejection, the bot waits `fak_backoff_ms` before attempting another trade. The daily loss gate checks current `daily_pnl` before candidate evaluation.

---

### Execution Configuration

```toml
"execution": {
  "slippage_tolerance": 0.01
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `slippage_tolerance` | float | `0.01` | Buy-price adjustment applied during execution: `buy_price = mid_price * (1 + slippage_tolerance)` (capped at `0.99`) |

Setting `0` disables this adjustment.

---

### Polling Configuration

```toml
"polling": {
  "signal_interval_ms": 1000,
  "status_interval_ms": 10000
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `signal_interval_ms` | integer | `1000` | Main signal loop interval in milliseconds |
| `status_interval_ms` | integer | `10000` | Status log printing interval in milliseconds |

This controls how often the bot checks for trading opportunities. Default 1000ms = 1 second.

**Note**: Other intervals are fixed:
- Settlement check: 15 seconds
- Market refresh: 60 seconds

---

## Configuration Validation

The bot validates the effective configuration on startup. Validation failures terminate startup with descriptive errors; config file load/parse failures first fall back to defaults, and those defaults are then validated.

### Validation Rules

| Field | Validation | Error Message |
|-------|------------|---------------|
| `signal_interval_ms` | > 0 | `polling.signal_interval_ms must be > 0` |
| `extreme_threshold` | 0 < value < 1 | `strategy.extreme_threshold must be in (0, 1)` |
| `extreme_threshold` | > fair_value | `strategy.extreme_threshold (X) must be > fair_value (Y)` |
| `fair_value` | 0 < value < 1 | `strategy.fair_value must be in (0, 1)` |
| `position_size_usdc` | > 0 | `strategy.position_size_usdc must be > 0` |
| `min_entry_price`, `max_entry_price` | `0 < min_entry_price < max_entry_price < 1` | `strategy min/max entry price must satisfy 0 < min < max < 1` |
| `min_ttl_for_entry_ms` | > 0 | `strategy.min_ttl_for_entry_ms must be > 0` |
| `symbol` | Binance format | `price_source.symbol must match Binance format like BTCUSDT` |

In addition, `extreme_threshold < 0.80` emits a startup warning since this bot targets extreme markets.

### Example Validation Errors

```bash
# Invalid symbol format
Error: price_source.symbol must match Binance format like BTCUSDT (got BTC-USD)

# Extreme threshold out of range
Error: strategy.extreme_threshold must be in (0, 1)

# Zero polling interval
Error: polling.signal_interval_ms must be > 0

# Extreme threshold <= fair_value
Error: strategy.extreme_threshold (0.40) must be > fair_value (0.50)
```

---

## Environment Variables

Some settings are loaded from environment variables (not stored in config.toml):

| Variable | Required | Description |
|----------|----------|-------------|
| `PRIVATE_KEY` | Required | Wallet private key for CLOB authentication and on-chain balance/redeem |
| `ALCHEMY_KEY` | Optional | Alchemy API key for Polygon RPC |

**Note**: Create a `.env` file in the repository root:

```bash
# .env
PRIVATE_KEY=0x...
ALCHEMY_KEY=...
```

---

## Configuration Examples

### Conservative Trading

```toml
{
  "strategy": {
    "extreme_threshold": 0.97,
    "fair_value": 0.50,
    "position_size_usdc": 1.0
  }
}
```

**Characteristics**:
- Higher extreme threshold (more selective)
- Same position size

### Aggressive Trading

```toml
{
  "strategy": {
    "extreme_threshold": 0.90,
    "fair_value": 0.50,
    "position_size_usdc": 2.0
  }
}
```

**Characteristics**:
- Lower extreme threshold (more trades)
- Larger position size

### Production Live Trading

```toml
{
  "market": {
    "stale_threshold_ms": 30000,
    "min_ttl_ms": 30000
  },
  "price_source": {
    "symbol": "BTCUSDT"
  },
  "strategy": {
    "extreme_threshold": 0.95,
    "fair_value": 0.50,
    "position_size_usdc": 5.0
  },
  "risk": {
    "max_fak_retries": 3,
    "fak_backoff_ms": 3000
  }
}
```

**Characteristics**:
- Real trades against CLOB
- Balanced settings
- Larger position size for production

---

## Runtime Configuration Updates

Configuration is loaded once at startup. To apply changes:

1. Stop the bot (Ctrl+C or SIGTERM)
2. Edit `config.toml`
3. Restart the bot

The bot does not support hot-reloading configuration.

---

## Configuration Best Practices

1. **Start with small position sizes** - Begin with $1 per trade while validating behavior
2. **Match symbol format** - Use Binance format (e.g. `BTCUSDT`) for the price source
3. **Validate before starting** - Run `cargo run` to check configuration
4. **Keep backups** - Version control your config.toml or keep backups
