# Configuration Guide

## Overview

The Polymarket 5m Bot uses a JSON configuration file (`config.json`) for all runtime settings. Configuration is loaded at startup and validated before the bot begins trading.

## Configuration File Location

- **Development**: `./config.json` (repository root)
- **Production**: Same location, but typically symlinked or mounted

## Creating Configuration

Copy the example configuration:

```bash
cp config.example.json config.json
```

Then edit `config.json` with your settings.

## Full Configuration Reference

```json
{
  "trading": {
    "mode": "paper"
  },
  "market": {
    "stale_threshold_ms": 30000,
    "min_ttl_ms": 30000
  },
  "polyclob": {
    "gamma_api_url": "https://gamma-api.polymarket.com"
  },
  "price_source": {
    "source": "binance",
    "symbol": "BTCUSDT"
  },
  "strategy": {
    "extreme_threshold": 0.8,
    "fair_value": 0.5,
    "position_size_usdc": 1.0
  },
  "risk": {
    "max_fok_retries": 3,
    "fok_backoff_ms": 3000
  },
  "polling": {
    "signal_interval_ms": 1000,
    "status_interval_ms": 10000
  }
}
```

## Section-by-Section Guide

### Trading Configuration

```json
"trading": {
  "mode": "paper"  // or "live"
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `mode` | string | `"paper"` | Trading mode: `"paper"` for simulation, `"live"` for real trading |

**Paper Mode**:
- Simulated trading with no real orders
- Uses local UUIDs as order IDs
- Starts with $100 simulated balance
- No private key required

**Live Mode**:
- Places real orders on Polymarket CLOB
- Requires `PRIVATE_KEY` environment variable
- Balance synced from on-chain USDC wallet
- Enables automatic CTF redemption

---

### Market Configuration

```json
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

```json
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

```json
"price_source": {
  "source": "binance",
  "symbol": "BTCUSDT"
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `source` | enum | `"binance"` | Price feed source: `"binance"`, `"binance_ws"`, `"coinbase"`, or `"coinbase_ws"` |
| `symbol` | string | `"BTCUSDT"` | Trading pair symbol (format depends on source) |

#### Source Options

| Source | Description | Symbol Format | Example |
|--------|-------------|---------------|---------|
| `binance` | Binance WebSocket (default) | No dash, uppercase | `BTCUSDT`, `ETHUSDT` |
| `binance_ws` | Binance WebSocket (explicit) | Same as above | `BTCUSDT` |
| `coinbase` | Coinbase WebSocket | With dash, uppercase | `BTC-USD`, `ETH-USD` |
| `coinbase_ws` | Coinbase WebSocket (explicit) | Same as above | `BTC-USD` |

**Symbol Format Validation**:
- The bot validates symbol format on startup
- Using wrong format for source causes startup error

#### Common Symbol Examples

**Binance**:
- `BTCUSDT` - Bitcoin / Tether
- `ETHUSDT` - Ethereum / Tether
- `SOLUSDT` - Solana / Tether

**Coinbase**:
- `BTC-USD` - Bitcoin / USD
- `ETH-USD` - Ethereum / USD
- `SOL-USD` - Solana / USD

---

### Strategy Configuration

```json
"strategy": {
  "extreme_threshold": 0.8,
  "fair_value": 0.5,
  "position_size_usdc": 1.0
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `extreme_threshold` | float | `0.80` | Market bias threshold to consider sentiment extreme (0.0-1.0) |
| `fair_value` | float | `0.50` | Fair-value assumption for binary outcome (0.0-1.0) |
| `position_size_usdc` | float | `1.0` | Fixed position size per trade in USDC |

#### Extreme Threshold

Determines when market sentiment is considered extreme:

```
if market_bias > 0.80 → Extremely bullish → Buy DOWN
if market_bias < 0.20 → Extremely bearish → Buy UP
otherwise → No trade
```

**Examples**:
- `0.80` (default): Trade when market is >80% or <20%
- `0.75`: More aggressive, trade at >75% or <25%
- `0.85`: More conservative, trade at >85% or <15%

#### Position Size

The bot uses a fixed position size per trade:

**Zero-share guard**: Orders resulting in 0 shares are rejected to prevent phantom trades.

**Minimum order**: Polymarket requires a minimum $1 order; shares are automatically bumped to meet this requirement.

---

### Risk Configuration

```json
"risk": {
  "max_fok_retries": 3,
  "fok_backoff_ms": 3000
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_fok_retries` | integer | `3` | Maximum FOK order rejections before giving up on market window |
| `fok_backoff_ms` | integer | `3000` | Milliseconds to wait after FOK rejection before retrying |

The FOK retry mechanism handles temporary liquidity issues when placing orders on the CLOB. After a rejection, the bot waits `fok_backoff_ms` before attempting another trade.

---

### Polling Configuration

```json
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

The bot validates configuration on startup. Invalid configurations cause immediate termination with descriptive error messages.

### Validation Rules

| Field | Validation | Error Message |
|-------|------------|---------------|
| `signal_interval_ms` | > 0 | `polling.signal_interval_ms must be > 0` |
| `extreme_threshold` | 0 < value < 1 | `strategy.extreme_threshold must be in (0, 1)` |
| `extreme_threshold` | > fair_value | `strategy.extreme_threshold (X) must be > fair_value (Y)` |
| `fair_value` | 0 < value < 1 | `strategy.fair_value must be in (0, 1)` |
| `position_size_usdc` | > 0 | `strategy.position_size_usdc must be > 0` |
| `symbol` | Source-specific format | `price_source.symbol must match...` |

### Example Validation Errors

```bash
# Invalid symbol format for source
Error: price_source.symbol must match Coinbase format like BTC-USD when source=coinbase (got BTCUSDT)

# Extreme threshold out of range
Error: strategy.extreme_threshold must be in (0, 1)

# Zero polling interval
Error: polling.signal_interval_ms must be > 0

# Extreme threshold <= fair_value
Error: strategy.extreme_threshold (0.40) must be > fair_value (0.50)
```

---

## Environment Variables

Some settings are loaded from environment variables (not stored in config.json):

| Variable | Required | Description |
|----------|----------|-------------|
| `PRIVATE_KEY` | Live mode only | Wallet private key for CLOB authentication |
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

```json
{
  "trading": { "mode": "paper" },
  "strategy": {
    "extreme_threshold": 0.85,
    "fair_value": 0.50,
    "position_size_usdc": 1.0
  }
}
```

**Characteristics**:
- Higher extreme threshold (more selective)
- Same position size

### Aggressive Trading

```json
{
  "trading": { "mode": "paper" },
  "strategy": {
    "extreme_threshold": 0.75,
    "fair_value": 0.50,
    "position_size_usdc": 2.0
  }
}
```

**Characteristics**:
- Lower extreme threshold (more trades)
- Larger position size

### Production Live Trading

```json
{
  "trading": { "mode": "live" },
  "market": {
    "stale_threshold_ms": 30000,
    "min_ttl_ms": 30000
  },
  "price_source": {
    "source": "binance",
    "symbol": "BTCUSDT"
  },
  "strategy": {
    "extreme_threshold": 0.80,
    "fair_value": 0.50,
    "position_size_usdc": 5.0
  },
  "risk": {
    "max_fok_retries": 3,
    "fok_backoff_ms": 3000
  }
}
```

**Characteristics**:
- Live mode (real trades)
- Balanced settings
- Larger position size for production

---

## Runtime Configuration Updates

Configuration is loaded once at startup. To apply changes:

1. Stop the bot (Ctrl+C or SIGTERM)
2. Edit `config.json`
3. Restart the bot

The bot does not support hot-reloading configuration.

---

## Configuration Best Practices

1. **Start with paper mode** - Test thoroughly before live trading
2. **Set reasonable position sizes** - Start small ($1-5 per trade)
3. **Match symbol to source** - Use correct format for your price source
4. **Validate before starting** - Run `cargo run` to check configuration
5. **Keep backups** - Version control your config.json or keep backups
