# Binance BTC 5m Value-Momentum Strategy

## Strategy in one sentence

Use the official Binance Prediction BTCUSDT opening price as the reference, estimate the chance that
Binance spot finishes on the same side at expiry, confirm with short-horizon Binance momentum, and
buy only when the executable Binance Prediction order book leaves conservative net edge.

This is neither a contrarian “fade 95%” rule nor a blind “buy any contract above 70%” rule.

## Market and price source

Before a market is eligible, the bot verifies Binance Prediction detail reports:

```text
symbol                  = BTCUSDT
variantData.type        = CRYPTO_UP_DOWN
priceFeedProvider       = BINANCE
priceFeedSymbol         = BTCUSDT
configured duration     ≈ 300 seconds
```

For each active market:

```text
K = Binance Prediction variantData.startPrice
S = latest Binance BTCUSDT WebSocket price
T = seconds until the Prediction market endDate
σ = Binance one-second realized spot volatility
```

The Paper settlement source is `variantData.endPrice`. Live settlement is Binance's settled-position
history for the exact token. The bot does not use a third-party exchange, oracle, or prediction API.

## Probability baseline

```text
move = (S - K) / K
z    = move / (σ × sqrt(T))
p_up = standard_normal_cdf(z)
p_down = 1 - p_up
```

All runtime math uses `rust_decimal::Decimal`; no financial calculation uses `f64`. The normal-CDF
baseline is intentionally transparent, but it is not yet statistically calibrated. This is why live
mode requires explicit `allow_uncalibrated_model_live = true` acknowledgement.

## Direction confirmation

```text
z > 0 and Binance 15s/30s momentum are both positive → evaluate UP
z < 0 and Binance 15s/30s momentum are both negative → evaluate DOWN
otherwise                                             → PASS
```

The Prediction contract price never decides direction. It only decides whether an already selected
direction is priced attractively enough.

## Entry window and gates

The checked-in Paper configuration allows new entries only with 75–150 seconds remaining. The
first half of each five-minute market is observation time; the final 75 seconds are excluded.

The decider stops at the first failed condition:

```text
1. available balance covers requested USDT plus advertised market fee
2. daily loss, loss-streak cooldown, daily trade cap, and circuit breaker allow entry
3. entry TTL is inside [min_entry_ttl_ms, max_entry_ttl_ms]
4. reference price, fresh spot price, and sufficient volatility samples are available
5. abs(z) >= min_normalized_move
6. Binance 15s and 30s momentum agree with z
7. selected Prediction book is fresh and has a bid
8. selected bid/ask spread <= max_spread
9. best ask notional >= position_size_usdt × min_depth_multiple
10. conservative net edge >= min_net_edge
11. all ask levels needed for the order are below the model value cap
12. no duplicate market position and unresolved-position limit is available
```

## Value calculation

The order book is walked, rather than reading a synthetic midpoint:

```text
effective_price = requested USDT / shares available across required asks

net_edge = model_probability
         - effective_price
         - model_uncertainty
         - fee_buffer
```

The maximum permissible price is:

```text
max_price = model_probability
          - model_uncertainty
          - fee_buffer
          - min_net_edge
```

The default requires at least five probability points of conservative net edge after a three-point
model buffer and a two-point fee/cost buffer.

## Binance MARKET/FOK execution

Binance Prediction market orders are `MARKET/FOK`, not FAK. The API has an approximate
minimum of 1.5 USDT for market orders, so the default requested size is 2 USDT.

For a Live entry:

```text
1. obtain Binance Get Quote for the selected token and fixed USDT amount
2. verify quote identity, expiry, configured slippage, and minReceive
3. require amountIn / minReceive <= max_price
4. submit MARKET/FOK with the returned quoteId
5. reconcile Binance order history using actual filled USDT, shares, and fees
```

If an acknowledgement arrives but the order cannot yet be reconciled, the bot persists that order
and blocks all future entries. It does not retry a potentially accepted order.

### Paper execution

Paper walks the current Binance Prediction asks, records the weighted effective entry price, and adds
an estimated fee from the market's advertised `feeRateBps`. It does not model queue priority or
network latency; those limitations must be considered during calibration.

## Position and risk controls

| Control | Checked-in value |
| --- | ---: |
| Requested order amount | 2 USDT |
| Entry window | 75–150 seconds left |
| Minimum normalized move | 0.60 |
| Minimum net edge | 0.05 |
| Model uncertainty | 0.03 |
| Fee buffer | 0.02 |
| Quote slippage | 25 bps |
| Maximum selected-side spread | 0.03 |
| Minimum top-ask depth | 5× requested amount |
| Maximum unresolved markets | 2 |
| Daily loss limit | 5 USDT |
| Daily trade cap | 8 |
| Loss streak | 3 losses → 30-minute cooldown |

There is one position per Binance Prediction market topic. Averaging down, martingale sizing, and
opposite-side micro-hedging are deliberately absent.

The circuit breaker (`circuit_breaker_window`/`circuit_breaker_min_win_rate`, checked-in 50 / 0.05)
is a catastrophe-only guard: it halts entries only after a full window of settled trades whose win
rate falls below the floor. The daily loss limit, loss-streak cooldown, and daily trade cap are the
primary controls. All of these counters are persisted to `account_state.json` while holding an
account lock, so under working storage a crash or restart does not reset the daily caps or the
loss-streak cooldown. If a state write fails the bot halts new entries and marks the state suspect;
recovery is operator-driven (see docs/ARCHITECTURE.md).

## Settlement and redemption

Paper holds until the official Binance `endPrice` is available. Live holds until Binance marks the
exact token settled. The bot does not use percentage-based contract-price stop losses, because those
would cross the Prediction spread and turn transient contract repricing into realized loss.

Live winners are submitted through Binance's batch-redeem endpoint. A failure is logged and can be
recovered explicitly with `binance-5m-tools --redeem <token-id>`.

## Calibration dataset

`logs/binance/<mode>/observations.csv` records every post-warmup evaluation:

```text
market topic / TTL
official reference price / current Binance spot price
realized volatility / normalized move / p_up
15s and 30s momentum
UP and DOWN bid, ask, effective price, and depth
pass reason or selected direction, probability, and edge
```

`outcomes.csv` writes official settlement labels and PnL for each entered market. Before enabling
live trading, collect at least 2,000 observed windows and 200 realistic Paper fills, then evaluate:

- walk-forward calibration and Brier score;
- net PnL after conservative fee/latency assumptions;
- drawdown and loss clustering;
- performance by TTL, volatility, price, and depth bucket;
- order-book availability and real fill assumptions.

Win rate alone is not evidence of positive expected value.
