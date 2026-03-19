# Trading Strategy

## 1. Core Idea

The Polymarket BTC 5-minute market is a binary-outcome market: `UP` means BTC ends the window higher, and `DOWN` means it ends lower.

The strategy does not try to predict short-term price direction directly. Instead, it looks for mispricing created by extreme market sentiment:

- Buy `DOWN` when the market becomes overly bullish
- Buy `UP` when the market becomes overly bearish
- Use `0.50` as the approximate fair value for a 5-minute window

```text
edge = fair_value - cheap_side_price
```

## 2. Signal Detection

The bot reads yes/no prices from Polymarket and computes market bias:

```text
mkt_up = yes_price / (yes_price + no_price)

if mkt_up > 0.80: buy DOWN
if mkt_up < 0.20: buy UP
else: no trade
```

The signal only moves forward if the cheaper side still has enough edge versus the fair-value assumption.

## 3. Decision Flow

```text
1. Require at least 60 BTC price samples in the local buffer
2. Reject stale Coinbase data older than 30 seconds
3. Wait until market tokens are available
4. Skip entries when less than 30 seconds remain before settlement
5. Check whether market sentiment is extreme enough
6. Check whether this 5-minute window has already been traded
7. Check whether risk controls still allow trading
8. Check whether edge clears the active threshold
9. Check whether the momentum filter allows a counter-trend entry
10. If all checks pass, size the position and place the order
```

The bot trades at most once per settlement window.

## 4. Momentum Filter

Extreme pricing can sometimes reflect a real short-term trend instead of crowd overreaction, so the strategy checks BTC momentum to avoid stepping directly in front of a strong move.

```text
momentum = (current_price - lookback_price) / lookback_price

if trade is DOWN and momentum > +threshold: skip
if trade is UP   and momentum < -threshold: skip
```

Default settings:

- `strategy.momentum_threshold = 0.001` (0.1%)
- `strategy.momentum_lookback_ms = 120000` (2 minutes)

## 5. Position Sizing

Position sizing uses a half-Kelly-style approximation based on historical win rate, then scales it by edge strength:

```text
win_rate = overall_win_rate.clamp(0.50, 0.75)
kelly_fraction = max(2 * win_rate - 1, 0.05)
half_kelly = kelly_fraction * 0.5

edge_multiplier = clamp(1 + (edge - 0.15) / 0.15, 1.0, 2.0)

size = balance * half_kelly * edge_multiplier
size = clamp(size, min_position, max_position)
size = min(size, balance * max_risk_fraction)
```

Default sizing caps include:

- `strategy.min_order_size = 5.0`
- `strategy.max_position_size = 50.0`
- `risk.max_risk_fraction = 0.10`

## 6. Risk Controls

| Mechanism | Rule |
| --- | --- |
| One trade per window | At most one trade per settlement window |
| Cooldown | At least `5000ms` between trades |
| Loss pause | Pause for 1 minute after 4-5 consecutive losses, 5 minutes after 6-7 |
| Circuit breaker | Stop trading after 8 consecutive losses |
| Daily stop | Stop when `daily_pnl <= -balance * max_daily_loss_pct` |
| Position cap | Never exceed `balance * max_risk_fraction` on a single trade |

Note: the code still defines `risk.max_daily_loss_usdc`, but the current decider logic actually enforces the percentage-based limit `risk.max_daily_loss_pct`.

## 7. Order Execution

### Paper Mode

- Does not send real orders to Polymarket
- Generates a local UUID as the order ID
- Records order cost as `size_usdc`

### Live Mode

- Uses an authenticated Polymarket CLOB client
- Computes `filled_shares = floor(size_usdc / price, 2)` and tracks the exact share count through settlement
- Actual cost is `filled_shares * price`, which may be slightly less than the requested `size_usdc` due to truncation
- Skips the trade if the FOK order cannot match or there is no liquidity

## 8. Settlement

Settlement is mode-dependent.

### Paper Mode

When a window expires, the bot performs a local settlement simulation. It prefers the Chainlink BTC/USD oracle on Polygon for settlement input, and falls back to the latest Coinbase price if the Chainlink query fails.

```text
btc_change = chainlink_btc_price - entry_btc_price

if abs(btc_change) < btc_tiebreaker_usd:
    btc_went_up = entry_price > 0.5
else:
    btc_went_up = btc_change > 0
```

The final win/loss decision is then computed from the position direction:

```text
if won:
    payout = filled_shares   (exact shares from execution, not reconstructed)
    pnl = payout - cost
else:
    payout = 0
    pnl = -cost
```

### Live Mode

Live mode does not use BTC-price simulation for final win/loss accounting. Instead, once the market is due, the bot queries Gamma for the exact market slug and waits for the market to be clearly resolved.

```text
Gamma market fields used:
- closed
- umaResolutionStatus
- outcomes
- outcomePrices

if market is resolved and Yes price == 1:
    winner = UP
if market is resolved and No price == 1:
    winner = DOWN
else:
    keep position pending and retry later
```

This keeps live accounting aligned with Polymarket resolution rather than local BTC price movement.

Both modes append settlement results to `logs/trades.csv`; the same file also receives trade-entry rows when positions are opened.

## 9. Live-Mode Redemption

In live mode, the bot initializes a CTF redeemer so it can redeem redeemable winning positions on-chain.

It also supports a manual command:

```bash
cargo run --release -- --redeem-all
```

This command scans the last 24 hours of markets and attempts redemption market by market.

## 10. Key Assumptions

1. A 5-minute BTC window can be approximated with a `0.50` fair value
2. Extreme market sentiment can create exploitable mispricing
3. Strong short-term trends can make extreme pricing justified, so the momentum filter matters
4. Live settlement should stay aligned with Polymarket resolution, while paper mode remains a fast local simulation
