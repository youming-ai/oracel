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

Example: market prices YES at 0.85, NO at 0.15
  → cheap side (NO/DOWN) = 0.15
  → edge = 0.50 - 0.15 = 0.35 (35%)
  → buy DOWN at 0.15, payout $1 per share if correct
```

## 2. Signal Detection

The bot reads yes/no prices from the Polymarket CLOB and computes market bias:

```text
mkt_up = yes_price / (yes_price + no_price)

if mkt_up > extreme_threshold (default 0.80): buy DOWN
if mkt_up < 1 - extreme_threshold (default 0.20): buy UP
otherwise: no trade (market is balanced)
```

The signal is a pre-filter in the main loop. If the market is not extreme, the bot skips the entire decision pipeline for that tick and logs `[IDLE] not_extreme`.

## 3. Decision Flow

Every tick (default 1 second), the bot runs through the following gates in order. If any gate rejects, the tick exits early.

```text
tick()
 │
 ├── 1. Buffer check: require ≥ 60 BTC price samples (~60 seconds of data)
 ├── 2. Staleness check: reject if latest Coinbase tick is older than 30 seconds
 ├── 3. Market readiness: skip if market token IDs have not been discovered yet
 ├── 4. Time-to-live: skip if < 30 seconds remain before settlement
 ├── 5. Extreme check: skip if market sentiment is not extreme (pre-filter)
 │
 └── decide()
      ├── 6. One-trade-per-window: skip if this settlement window was already traded
      ├── 7. Risk checks: balance > 0, cooldown elapsed, no loss pause, no circuit breaker, daily loss within limit
      ├── 8. Market data: skip if yes or no price is missing or ≤ 0.01
      ├── 9. Edge threshold: skip if edge < edge_threshold_early (default 15%)
      ├── 10. Momentum filter: skip if BTC trend contradicts the trade direction
      │
      └── TRADE: size the position and execute
```

The bot trades at most once per 5-minute settlement window.

## 4. Momentum Filter

Extreme pricing can sometimes reflect a real short-term trend instead of crowd overreaction. The strategy checks BTC momentum over a lookback window to avoid stepping in front of a strong move.

```text
momentum = (current_btc_price - lookback_btc_price) / lookback_btc_price

if trade is DOWN and momentum > +threshold: skip (BTC is rising, DOWN bet is against trend)
if trade is UP   and momentum < -threshold: skip (BTC is falling, UP bet is against trend)
```

Default settings:

- `strategy.momentum_threshold = 0.001` (0.1%)
- `strategy.momentum_lookback_ms = 120000` (2 minutes)

At BTC ~$70,000, a 0.1% threshold corresponds to approximately $70 of price movement over 2 minutes.

## 5. Position Sizing

Position sizing uses a fixed fraction of balance:

```text
size_usdc = max(balance / 100, 1)
```

This allocates 1% of the current balance per trade, with a $1 floor. The approach is intentionally conservative: each losing trade costs roughly 1% of equity, while winners at extreme prices return far more due to the cheap entry.

Example at different balance levels:

| Balance | Size (USDC) | At price 0.07 | Shares | Win payout | Loss |
| --- | --- | --- | --- | --- | --- |
| $1,000 | $10 | $10 / 0.07 | 142 | $142 | -$10 |
| $100 | $1 | $1 / 0.07 | 14 | $14 | -$1 |

Shares are floored to whole numbers so that the CLOB order amount stays within its 2-decimal-place precision limit.

## 6. Risk Controls

| Mechanism | Rule | Config |
| --- | --- | --- |
| One trade per window | At most one trade per 5-minute settlement window | — |
| Cooldown | Minimum `cooldown_ms` between any two trades | `risk.cooldown_ms` (default 5000) |
| Loss pause | 1-minute pause after 4–5 consecutive losses; 5-minute pause after 6–7 | — |
| Circuit breaker | Stop trading entirely after N consecutive losses | `risk.max_consecutive_losses` (default 8) |
| Daily stop | Stop when `daily_pnl ≤ -(balance × max_daily_loss_pct)` | `risk.max_daily_loss_pct` (default 0.10) |
| Balance guard | No trading when balance ≤ 0 | — |

Daily PnL resets automatically at midnight UTC.

## 7. Order Execution

### Paper Mode

- Does not send real orders to Polymarket
- Generates a local UUID as the order ID
- Tracks the same `filled_shares` and `cost` fields as live mode

### Live Mode

- Uses an authenticated Polymarket CLOB client (SDK-based)
- Places a Fill-or-Kill (FOK) limit buy at the current mid price
- Computes `filled_shares = floor(size_usdc / price)` and sends that as the order size
- Actual cost is `filled_shares × price`, which may be slightly less than the requested `size_usdc` due to floor truncation
- If the FOK order is rejected (no liquidity at the requested price), the trade is skipped gracefully
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
cargo run --release -- --redeem-all
```

This scans the last 24 hours of 5-minute markets (288 windows) and attempts redemption for any positions held on-chain.

## 10. State Persistence

The bot persists the following to `logs/<mode>/state.json` after every trade and settlement:

- Pending positions (direction, cost, shares, settlement time, condition ID)
- Last traded settlement timestamp (prevents duplicate trades on restart)
- Consecutive win/loss streaks
- Loss pause timer
- Daily PnL and reset date

On startup, the bot restores this state to continue seamlessly after a restart.

## 11. Key Assumptions

1. A 5-minute BTC window is approximately a coin flip (`fair_value = 0.50`), since short-term BTC price movements are dominated by noise rather than directional signal
2. Extreme market sentiment (>80% on one side) creates exploitable mispricing because the crowd overestimates the probability of a directional move
3. Strong short-term BTC trends can justify extreme pricing, so the momentum filter is essential to avoid trading against genuine moves
4. Settlement is based on Polymarket's official resolution via Gamma API, ensuring accurate accounting in both paper and live modes
5. Conservative position sizing (1% per trade) ensures survivability through losing streaks while still capturing asymmetric payoffs at extreme prices
