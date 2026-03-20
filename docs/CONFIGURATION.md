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
    "window_minutes": 5.0
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
    "btc_tiebreaker_usd": 5.0,
    "momentum_threshold": 0.001,
    "momentum_lookback_ms": 120000
  },
  "edge": {
    "edge_threshold_early": 0.15
  },
  "risk": {
    "max_consecutive_losses": 8,
    "max_daily_loss_pct": 0.10,
    "cooldown_ms": 5000,
    "enforce_limits": false
  },
  "polling": {
    "signal_interval_ms": 1000
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
- Starts with $1,000 simulated balance
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
  "window_minutes": 5.0
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `window_minutes` | float | `5.0` | Duration of each trading window in minutes |

This should match the Polymarket market duration (typically 5 minutes for BTC up/down markets).

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
  "btc_tiebreaker_usd": 5.0,
  "momentum_threshold": 0.001,
  "momentum_lookback_ms": 120000
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `extreme_threshold` | float | `0.80` | Market bias threshold to consider sentiment extreme (0.0-1.0) |
| `fair_value` | float | `0.50` | Fair-value assumption for binary outcome (0.0-1.0) |
| `btc_tiebreaker_usd` | float | `5.0` | BTC price change threshold (legacy, largely unused) |
| `momentum_threshold` | float | `0.001` | BTC momentum threshold (0.1%) to filter counter-trend trades |
| `momentum_lookback_ms` | integer | `120000` | Momentum lookback window in milliseconds (2 minutes) |

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

#### Momentum Filter

Prevents trading against strong trends:

```
momentum_threshold = 0.001 (0.1%)
lookback = 120000ms (2 minutes)

If BTC moved >0.1% in 2 minutes:
- Trading UP when BTC is falling → BLOCKED
- Trading DOWN when BTC is rising → BLOCKED
```

---

### Edge Configuration

```json
"edge": {
  "edge_threshold_early": 0.15
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `edge_threshold_early` | float | `0.15` | Minimum edge required to trade (15%) |

#### Edge Calculation

```
edge = fair_value - cheap_side_price

Example:
- Market: YES 0.85, NO 0.15
- Fair value: 0.50
- Cheap side: NO at 0.15
- Edge = 0.50 - 0.15 = 0.35 (35%)
- 35% > 15% threshold → TRADE
```

---

### Risk Configuration

```json
"risk": {
  "max_consecutive_losses": 8,
  "max_daily_loss_pct": 0.10,
  "cooldown_ms": 5000,
  "enforce_limits": false
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_consecutive_losses` | integer | `8` | Circuit breaker threshold for consecutive losses |
| `max_daily_loss_pct` | float | `0.10` | Daily loss limit as fraction of balance (10%) |
| `cooldown_ms` | integer | `5000` | Minimum milliseconds between trades |
| `enforce_limits` | boolean | `false` | If `true`, cooldown and daily loss become hard blocks |

#### Risk Control Modes

**Advisory Mode** (`enforce_limits: false`):
```
Cooldown active: "[RISK] Cooldown active... trading continues"
Daily loss exceeded: "[RISK] Daily loss... trading continues"
→ Trading continues, warnings logged
```

**Strict Mode** (`enforce_limits: true`):
```
Cooldown active: "[RISK] Cooldown active... blocking trade"
Daily loss exceeded: "[RISK] Daily loss... blocking trade"
→ Trading blocked until condition clears
```

**Always Blocks** (regardless of `enforce_limits`):
- Insufficient balance (≤ 0)
- Already traded this window
- Invalid market data

#### Consecutive Losses

Pause durations based on consecutive losses:

| Losses | Pause Duration | Note |
|--------|---------------|------|
| 0-3 | None | Normal operation |
| 4-5 | 1 minute | Advisory warning only |
| 6-7 | 5 minutes | Advisory warning only |
| 8+ | 15 minutes | Circuit breaker (advisory) |

---

### Polling Configuration

```json
"polling": {
  "signal_interval_ms": 1000
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `signal_interval_ms` | integer | `1000` | Main signal loop interval in milliseconds |

This controls how often the bot checks for trading opportunities. Default 1000ms = 1 second.

**Note**: Other intervals are fixed:
- Settlement check: 15 seconds
- Market refresh: 60 seconds
- Status log: 10 seconds

---

## Configuration Validation

The bot validates configuration on startup. Invalid configurations cause immediate termination with descriptive error messages.

### Validation Rules

| Field | Validation | Error Message |
|-------|------------|---------------|
| `signal_interval_ms` | > 0 | `polling.signal_interval_ms must be > 0` |
| `extreme_threshold` | 0 < value < 1 | `strategy.extreme_threshold must be in (0, 1)` |
| `fair_value` | 0 < value < 1 | `strategy.fair_value must be in (0, 1)` |
| `max_daily_loss_pct` | 0 < value ≤ 1 | `risk.max_daily_loss_pct must be in (0, 1]` |
| `window_minutes` | > 0 | `market.window_minutes must be > 0` |
| `symbol` | Source-specific format | `price_source.symbol must match...` |

### Example Validation Errors

```bash
# Invalid symbol format for source
Error: price_source.symbol must match Coinbase format like BTC-USD when source=coinbase (got BTCUSDT)

# Extreme threshold out of range
Error: strategy.extreme_threshold must be in (0, 1)

# Zero polling interval
Error: polling.signal_interval_ms must be > 0
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
    "momentum_threshold": 0.002
  },
  "edge": {
    "edge_threshold_early": 0.20
  },
  "risk": {
    "enforce_limits": true,
    "cooldown_ms": 10000,
    "max_daily_loss_pct": 0.05
  }
}
```

**Characteristics**:
- Higher extreme threshold (more selective)
- Stronger momentum filter (avoid trends)
- Higher edge requirement (better value)
- Strict risk controls
- Longer cooldown

### Aggressive Trading

```json
{
  "trading": { "mode": "paper" },
  "strategy": {
    "extreme_threshold": 0.75,
    "momentum_threshold": 0.0005
  },
  "edge": {
    "edge_threshold_early": 0.10
  },
  "risk": {
    "enforce_limits": false,
    "cooldown_ms": 1000,
    "max_daily_loss_pct": 0.20
  }
}
```

**Characteristics**:
- Lower extreme threshold (more trades)
- Weaker momentum filter (more opportunities)
- Lower edge requirement (more trades)
- Advisory risk controls
- Shorter cooldown

### Production Live Trading

```json
{
  "trading": { "mode": "live" },
  "price_source": {
    "source": "binance",
    "symbol": "BTCUSDT"
  },
  "strategy": {
    "extreme_threshold": 0.80,
    "fair_value": 0.50,
    "momentum_threshold": 0.001,
    "momentum_lookback_ms": 120000
  },
  "edge": {
    "edge_threshold_early": 0.15
  },
  "risk": {
    "max_consecutive_losses": 10,
    "max_daily_loss_pct": 0.15,
    "cooldown_ms": 5000,
    "enforce_limits": false
  }
}
```

**Characteristics**:
- Live mode (real trades)
- Balanced settings
- Advisory risk controls (opportunity capture)
- Reasonable daily loss limit

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
2. **Use advisory mode initially** - Let `enforce_limits: false` while learning
3. **Set reasonable daily loss limits** - Protect capital (5-15% recommended)
4. **Match symbol to source** - Use correct format for your price source
5. **Validate before starting** - Run `cargo run` to check configuration
6. **Keep backups** - Version control your config.json or keep backups
