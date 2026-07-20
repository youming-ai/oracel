# Trading Strategy

## Strategy in one sentence

Estimate the probability of the Chainlink BTC/USD close finishing above the same feed's five-minute
opening price, confirm direction with low-latency Binance momentum, and buy only when the executable
Polymarket order book leaves a conservative net edge.

This is an oracle-aligned value-momentum strategy. It is not the former 95% contrarian strategy and
it does not infer direction from the prediction-market price.

## Source of truth

Polymarket's BTC five-minute rules resolve UP when the ending Chainlink BTC/USD Data Stream value is
greater than or equal to its opening value. Otherwise they resolve DOWN.

| Source | Role |
| --- | --- |
| Polymarket RTDS `crypto_prices_chainlink` | Authoritative opening/current price and volatility |
| Binance BTCUSDT WebSocket | Faster 15-second and 30-second momentum confirmation |
| Polymarket CLOB order book | Executable bid, ask, depth, average fill, and limit price |
| Gamma API | Market discovery and official resolution state |

A Chainlink opening tick must exist within five seconds of the exact window boundary. The bot skips
the market rather than substituting Binance or a late oracle value.

## Probability model

For each tick:

```text
K       = Chainlink opening price
S       = latest Chainlink price
sigma   = one-second realized volatility over the configured lookback
T       = seconds remaining
move    = (S - K) / K
z       = move / (sigma * sqrt(T))
p_up    = standard_normal_cdf(z)
p_down  = 1 - p_up
```

All runtime calculations use `rust_decimal::Decimal`. The current normal-CDF model is a transparent
baseline, not a historically calibrated claim. That is why live mode requires an explicit
`allow_uncalibrated_model_live = true` acknowledgement.

The first release collects `observations.csv` so this baseline can be replaced by a walk-forward
empirical calibration without changing execution or accounting.

## Entry gates

The decider returns on the first failed gate:

```text
1. available balance covers position_size_usdc
2. daily loss and rolling-result circuit breakers permit trading
3. min_entry_ttl_ms <= TTL <= max_entry_ttl_ms
4. opening/current Chainlink prices and realized volatility are available
5. abs(z) >= min_normalized_move
6. Binance 15s and 30s trends agree with the Chainlink direction
7. selected outcome has a fresh executable order-book quote and a bid
8. selected bid/ask spread <= max_spread
9. best-ask notional >= position_size_usdc * min_depth_multiple
10. conservative net edge >= min_net_edge
11. required fill levels do not exceed the value-capped order limit
```

The checked-in entry window is 75–150 seconds before expiry. The first half of the market is for
observation; the final 75 seconds are excluded from new entries.

## Value and order limit

The CLOB client walks asks for the configured fixed-USDC amount:

```text
effective_price = requested USDC / shares available across required ask levels
```

The conservative edge is:

```text
net_edge = model_probability
         - effective_price
         - model_uncertainty
         - fee_buffer
```

The checked-in minimum is 0.05. The largest acceptable live FAK limit is:

```text
order_limit = model_probability
            - model_uncertainty
            - fee_buffer
            - min_net_edge
```

The executor rounds this cap down. It never raises a quote by an unconditional slippage percentage,
because doing so could turn a positive-value signal negative.

`fee_buffer` is currently a conservative probability-point allowance rather than a reconstruction of
Polymarket's dynamic fee formula. Actual CLOB-reported cost and shares remain authoritative for live
accounting.

## Direction and momentum

```text
z > 0 and both Binance trends > 0  -> evaluate UP
z < 0 and both Binance trends < 0  -> evaluate DOWN
otherwise                          -> PASS
```

Market prices never create direction. A 95% contract may be correctly priced and is neither an
automatic momentum entry nor an automatic reversal entry.

## Position and execution

- Fixed requested amount: `$1` with the checked-in configuration.
- Minimum Polymarket order amount: `$1`.
- At most one position per condition/market.
- At most two unresolved positions globally, allowing delayed Gamma settlement without duplicate
  market exposure.
- No averaging down, martingale sizing, or opposite-side micro-hedge.
- Paper uses the order-book weighted effective price and assumes the validated `$1` amount fills.
- Live submits a fixed-USDC FAK capped by model value and records only actual CLOB cost and shares.
- A rejection or zero fill creates no position.

The first version holds positions through official settlement. It does not use a contract-price
percentage stop, which would cross the spread and confuse market repricing with final-outcome risk.

## Risk controls

Checked-in Paper defaults:

| Control | Value |
| --- | ---: |
| Position size | `$1` |
| Daily loss limit | `$5` |
| Maximum trades per day | `8` |
| Loss-streak cooldown | `3 losses → 30 minutes` |
| Entry TTL | `75–150s` |
| Minimum normalized move | `0.60` |
| Minimum net edge | `0.05` |
| Model uncertainty | `0.03` |
| Fee buffer | `0.02` |
| Maximum spread | `0.03` |
| Minimum best-ask depth | `5x` order size |
| Maximum unresolved positions | `2` |

A three-loss streak pauses entries for 30 minutes; a win clears the streak. Daily trade count, daily
PnL, and rolling win-rate history remain in memory and are not restored after restart. Balance and
pending positions are persisted.

## Observation and calibration

Every model evaluation after market and price warm-up is appended to
`logs/<mode>/observations.csv`, including:

- Chainlink opening/current price and realized volatility;
- Binance price and 15s/30s trend;
- both outcomes' bid, ask, effective price, and best-ask depth;
- TTL, pass reason, selected direction, model probability, and net edge.

`trades.csv` remains the accounting ledger. `outcomes.csv` records official settlement, direction,
PnL, and entry/settlement Chainlink values keyed by market slug so observations can be calibrated
without reconstructing labels from unstructured logs.

Before removing the uncalibrated-live guard, collect at least 2,000 market windows and 200 realistic
Paper fills, join each observation to its official outcome, and evaluate walk-forward calibration,
net PnL after conservative costs, drawdown, and performance by TTL/volatility/price bucket. Win rate
alone is not a sufficient metric.

## Known limitations

- The normal-CDF probability is not yet empirically calibrated.
- Paper validates current depth but does not model network latency or partial fills.
- Dynamic fees are represented by a conservative fixed buffer.
- Chainlink history delivered on a fresh RTDS connection may not reach the current window boundary;
  the bot safely waits for a later market in that case.
- Daily PnL and rolling circuit-breaker history reset on process restart.
- Automatic redemption retries are not persisted after accounting settlement.
