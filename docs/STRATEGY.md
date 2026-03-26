# Trading Strategy

## 1. Core Idea

The Polymarket BTC 5-minute market is a binary-outcome market: `UP` means BTC ends the window higher, and `DOWN` means it ends lower.

The strategy does not try to predict short-term price direction directly. Instead, it exploits mispricing created by extreme market sentiment:

- Buy `DOWN` when the market becomes overly bullish (crowd too confident BTC goes up)
- Buy `UP` when the market becomes overly bearish (crowd too confident BTC goes down)
- Use `0.50` as the approximate fair value, since BTC is roughly equally likely to go up or down in any given 5-minute window

The edge comes from buying the cheap side of the market when the crowd overreacts:

```text
edge = fair_value - cheap_side_price

Example: market prices YES at 0.88, NO at 0.12
  → cheap side (NO/DOWN) = 0.12
  → edge = 0.50 - 0.12 = 0.38 (38%)
  → buy DOWN at 0.12, payout $1 per share if correct
```

## 2. Price Sources

The bot supports multiple price feeds via WebSocket:

- **Binance** (default): `BTCUSDT` via `wss://stream.binance.com:9443/ws`
- **Coinbase**: `BTC-USD` via Coinbase Advanced Trade WebSocket

Configuration in `config.json`:
```json
{
  "price_source": {
    "source": "binance",
    "symbol": "BTCUSDT"
  }
}
```

The bot maintains a rolling buffer of price ticks for momentum calculations. WebSocket connections automatically reconnect on disconnection.

## 3. Signal Detection

The bot reads yes/no prices from the Polymarket CLOB and computes market bias:

```text
mkt_up = yes_price / (yes_price + no_price)

if mkt_up > extreme_threshold (default 0.95): create DOWN candidate
if mkt_up < 1 - extreme_threshold (default 0.05): create UP candidate
otherwise: no trade (market is balanced)
```

An extreme quote is only the first gate. It creates a candidate trade that must pass additional entry filters before execution.

### Spread Check

The bot validates market liquidity by checking the spread.

```text
if yes_price + no_price < 0.80:
    skip trade (wide spread indicates unreliable mid prices)
```

When the spread is too wide (>20%), mid prices are unreliable and the bot skips to avoid adverse fills.

## 4. Decision Flow

Every tick (default 1 second), the bot runs through the following gates in order. If any gate rejects, the tick exits early.

```text
tick()
 │
 ├── 1. Buffer check: require ≥ 60 BTC price samples (~60 seconds of data)
 ├── 2. Staleness check: reject if latest price tick is older than 30 seconds
 ├── 3. Market readiness: skip if market token IDs have not been discovered yet
 ├── 4. Time-to-live: skip if < 30 seconds remain before settlement
 │
 └── decide()
      ├── 5. Balance check: skip if balance ≤ 0
      ├── 6. Market data: skip if yes or no price is missing or ≤ 0.01
      ├── 7. Spread check: skip if yes + no < 0.80 (wide spread)
      ├── 8. Extreme check: evaluate extreme threshold inside decide()
      ├── 9. Candidate creation: extreme quote creates directional candidate
      ├── 10. Min-edge gate: require fair_value - candidate_entry_price >= min_edge
      ├── 11. Entry price range: candidate quote must be within strategy min/max
      ├── 12. Strategy TTL floor: candidate must satisfy min_ttl_for_entry_ms
      ├── 13. Spot momentum confirmation: candidate must pass 30s and 60s checks
      │
      └── TRADE: size the position and execute only after all candidate filters pass
```

There is no separate pre-decide extreme gate in `tick()`: the extreme filter runs inside `decide()`, where a qualifying quote becomes a trade candidate and then passes the remaining entry filters.

The bot trades at most once per 5-minute settlement window.

### Spot Momentum Confirmation

Entry confirmation uses directional spot momentum over two windows:

```text
momentum_30s = latest_spot_price - spot_price_30s_ago
momentum_60s = latest_spot_price - spot_price_60s_ago
```

Units are spot price points (USD price delta on BTC spot), not percentages.

- For `DOWN` candidates, reject when momentum is accelerating up (positive 30s/60s deltas above configured thresholds)
- For `UP` candidates, reject when momentum is accelerating down (negative 30s/60s deltas beyond configured thresholds in absolute direction)

## 5. Position Sizing

Position sizing starts from the configured target amount per trade:

```text
shares = floor(position_size_usdc / entry_price)
cost = shares * entry_price

if cost < 1.0:
    shares = ceil(1.0 / entry_price)
    cost = shares * entry_price
```

Example at different entry prices:

| Entry Price | Configured Size | Initial floor shares | Final shares | Final cost | Win payout | Loss |
| --- | --- | --- | --- | --- | --- | --- |
| 0.08 | $1.0 | floor(1.0 / 0.08) = 12 | 13 (bumped) | $1.04 | $13 | -$1.04 |
| 0.12 | $0.75 | floor(0.75 / 0.12) = 6 | 9 (bumped) | $1.08 | $9 | -$1.08 |

Shares are floored to whole numbers so that the CLOB order amount stays within its 2-decimal-place precision limit. If the resulting cost is below $1, shares are bumped to `ceil(1 / entry_price)`, so final cost can exceed configured `position_size_usdc`. **Orders resulting in 0 shares at the initial floor step are rejected** to prevent phantom trades.

## 6. Risk Controls

The bot implements basic risk controls:

| Mechanism | Rule | Behavior |
| --- | --- | --- |
| One trade per window | At most one trade per 5-minute settlement window | Hard limit |
| Zero balance guard | Reject trades when balance ≤ 0 | Hard block |
| Daily loss limit gate | If `daily_loss_limit_usdc > 0` and `daily_pnl < -daily_loss_limit_usdc`, skip new trades | Hard block (`0` disables) |
| FAK retries | Retry failed FAK orders up to `max_fak_retries` times with `fak_backoff_ms` delay | Automatic retry |

**Note**: The bot focuses on capturing opportunities in the brief 5-minute window. Balance is the primary protection mechanism.

## 7. Order Execution

### Paper Mode

- Does not send real orders to Polymarket
- Generates a local UUID as the order ID
- Applies the same slippage-adjusted buy price as live mode for simulated fills/costs
- Tracks the same `filled_shares` and `cost` fields as live mode

### Live Mode

- Uses an authenticated Polymarket CLOB client (SDK-based)
- Places a Fill-and-Kill (FAK) limit buy at the same slippage-adjusted price used in paper mode: `buy_price = mid_price * (1 + slippage_tolerance)` (capped at `0.99`)
- Computes `filled_shares = floor(size_usdc / price)` and sends that as the order size
- **Zero-share guard**: If computed shares == 0, the order is rejected
- Actual cost is `filled_shares × price`, which may be slightly less than `effective_size_usdc` due to floor truncation, or higher than configured `position_size_usdc` when the order is bumped to the $1 minimum
- If the FAK order is rejected (no liquidity at the requested price), the trade is skipped gracefully
- The on-chain USDC balance is re-queried every tick to keep local accounting in sync with the wallet

### Extreme Price Guard

Both modes skip execution if the target price is ≤ 0.01 or ≥ 0.99. This prevents placing orders at degenerate prices where the payout ratio collapses.

## 8. Settlement

Both paper and live modes use the Gamma API to check market resolution state. The settlement checker polls every 15 seconds for pending positions.

Resolution detection:
```text
1. umaResolutionStatus contains "resolved"
2. closed == true
3. outcomePrices shows one outcome at 1.0 and the other at 0.0

if resolved and Up/Yes price == 1  → winner = UP
if resolved and Down/No price == 1 → winner = DOWN
otherwise → keep position pending and retry on next check
```

Payout calculation:
```text
if won:
    payout = filled_shares    (each share pays $1)
    pnl = payout - cost
else:
    payout = 0
    pnl = -cost
```

Both modes append settlement results to `logs/<mode>/trades.csv`.

## 9. Live-Mode Redemption

In live mode, the bot initializes a CTF (Conditional Tokens Framework) redeemer that can redeem winning positions on-chain after resolution.

Automatic flow:

1. When a position settles as a win, it is queued for on-chain redemption
2. The redeemer checks `payoutDenominator` and `balanceOf` on the CTF contract to confirm the position is redeemable
3. If redeemable, it calls `redeemPositions` on-chain
4. If not yet redeemable (resolution may take time to propagate), the redeemer retries up to 10 times (~5 minutes)

Manual redemption for historical markets:

```bash
cargo run --release --bin polybot-tools -- --redeem-all
```

This scans the last 24 hours of 5-minute markets (288 windows) and attempts redemption for any positions held on-chain.

## 10. State Management

The bot uses in-memory state only. Balance is persisted to `logs/<mode>/balance` (atomic write via tmp+rename). Since markets are 5-minute windows, any pending positions from a previous run will have already settled by the time the bot restarts — no state.json persistence is needed.

## 11. Key Assumptions

1. A 5-minute BTC window is approximately a coin flip (`fair_value = 0.50`), since short-term BTC price movements are dominated by noise rather than directional signal
2. Extreme market sentiment (≥95% on one side) creates exploitable mispricing because the crowd overestimates the probability of a directional move
3. Settlement is based on Polymarket's official resolution via Gamma API, ensuring accurate accounting in both paper and live modes
4. Fixed position sizing keeps the strategy simple and predictable
5. **Balance is the primary protection**: The bot rejects trades when balance is zero or negative
