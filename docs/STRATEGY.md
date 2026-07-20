# Trading Strategy

## Strategy in one sentence

When Polymarket becomes extremely confident in one BTC five-minute outcome, buy the cheaper opposite
outcome, subject to liquidity, price, time, momentum, balance, and loss controls.

This is a contrarian market-pricing strategy. Binance BTC data is a filter, not the primary signal.

## Inputs

| Input | Use |
| --- | --- |
| Polymarket YES buy quote | Detect extreme UP sentiment and price an UP entry |
| Polymarket NO buy quote | Detect extreme DOWN sentiment and price a DOWN entry |
| Remaining market TTL | Reject entries too close to expiry |
| Binance BTC trend | Reject trades fighting strong recent momentum |
| Account state | Balance, daily loss, and rolling result controls |

The code uses raw CLOB buy quotes. It does not normalize YES and NO into a synthetic probability.

## Direction

```text
YES > extreme_threshold  → candidate = buy DOWN at the NO quote
NO  > extreme_threshold  → candidate = buy UP at the YES quote
otherwise                → PASS not_extreme
```

With the checked-in configuration, `extreme_threshold = 0.95`. The comparison is strict (`>`), not
`>=`.

## Decision gates

The decider evaluates gates in this order and returns immediately on the first failure:

```text
1. balance >= position_size_usdc
2. daily loss limit not reached
3. YES and NO quotes both exist and are > 0.01
4. one raw quote is above extreme_threshold
5. abs((YES + NO) - 1) <= 0.06
6. opposite-side entry price is within [min_entry_price, max_entry_price]
7. remaining TTL >= min_ttl_for_entry_ms
8. Binance trend is not strongly against the candidate direction
9. circuit-breaker win rate is not below its floor
```

Outside the decider, execution also requires:

- a warm Binance buffer;
- a non-stale latest BTC tick;
- a discovered Polymarket market;
- no existing pending position; and
- live FAK retry/backoff allowance.

## Price and payoff metrics

```text
edge         = fair_value - entry_price
payoff_ratio = (1 - entry_price) / entry_price
```

`edge` and `payoff_ratio` are recorded and displayed. There is currently no separate minimum-edge
gate; the configured entry-price range indirectly constrains edge.

`fair_value = 0.50` is a strategy assumption, not an oracle-derived probability.

## Momentum filter

`PriceSource` compares the latest Binance price with the oldest available tick at or after the
configured lookback cutoff:

```text
trend_pct = (latest - old) / old * 100
```

- DOWN candidate passes unless BTC is rising by more than `btc_trend_min_pct`.
- UP candidate passes unless BTC is falling by more than `btc_trend_min_pct`.
- Set `btc_trend_window_s = 0` to make the calculated trend zero and effectively disable the filter.

Momentum never creates a trade on its own.

## Position sizing and fills

`position_size_usdc` is the maximum requested spend and must be at least 1 USDC.

### Paper

```text
requested cost = position_size_usdc
shares         = truncate(position_size_usdc / max_buy_price, 4 decimals)
entry price    = cost / shares
```

Paper assumes a complete fill. It does not model queue position, depth, fees, latency, or partial
fills.

### Live

The executor submits a fixed-USDC FAK market buy with the slippage-adjusted price as its maximum
price. It records `making_amount` as actual cost and `taking_amount` as actual filled shares from the
CLOB response. Zero-fill and rejected FAK responses do not create positions.

## Slippage

```text
max_buy_price = round_2dp(mid_quote * (1 + slippage_tolerance))
```

The value is capped at `0.99`. Both modes reject target prices at or below `0.01` and at or above
`0.99` before applying the order.

The CLOB request explicitly asks for the BUY-side price. Strategy analysis should treat it as an
executable quote, not a mathematically derived mid.

## Risk controls

| Control | Current behavior |
| --- | --- |
| Position budget | Fixed USDC amount per entry |
| Open-position limit | One pending position globally |
| Daily loss | Blocks at or below the configured negative limit; `0` disables |
| Momentum | Blocks entries against a strong Binance trend |
| Spread | Blocks when YES + NO differs from 1 by more than 6% |
| Entry range | Avoids prices outside the configured cheap-side range |
| TTL | Requires enough time before market expiry |
| Circuit breaker | Blocks when recent-result win rate is below the floor |
| Live FAK retry | Per-market retry cap plus backoff |

The result history is kept in memory with a 200-result cap. W/L counters and circuit-breaker history
are not restored after restart; balance and pending positions are restored.

## Why the default strategy trades rarely

The checked-in defaults require all of the following at once:

- one side strictly above 95%;
- the opposite side priced between 0.05 and 0.15;
- at least 90 seconds remaining;
- 60 Binance ticks already buffered; and
- no conflicting BTC momentum.

Extreme prices often occur late in a five-minute window, when the TTL gate already blocks entry.
Zero paper trades over a short run is therefore expected and does not by itself indicate a runtime
failure. Use `RUST_LOG=debug` to see the first failed gate.

## Settlement

Trading direction is settled from Gamma's official Polymarket resolution, not by comparing Binance
prices locally.

```text
win:  payout = filled_shares; pnl = payout - cost
loss: payout = 0;             pnl = -cost
```

Live winners additionally require on-chain CTF redemption before the wallet receives spendable USDC.

## Known model limitations

- The 0.50 fair-value assumption is not empirically calibrated in code.
- Fees are not included in the decision edge.
- Paper fills do not model order-book depth or partial execution.
- A delayed settlement blocks trading in subsequent windows because only one global pending position
  is allowed.
- The circuit breaker uses the bounded in-memory result history and resets on restart.
- Automatic redemption retries are not persisted after accounting settlement.

These are explicit strategy changes for a later phase; they are not hidden configuration switches.
