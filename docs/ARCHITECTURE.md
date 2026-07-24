# Binance Prediction Trading Flow

Both Paper and Live mode use Binance data, the same decision model, risk gates, persisted state, and
settlement flow. Only fill simulation versus authenticated order submission differs.

## End-to-end flow

```text
Startup
  ├─ load config.toml and Binance API credentials from .env
  ├─ validate Binance Prediction settings and Paper/Live guardrails
  ├─ select logs/binance/paper or logs/binance/live
  ├─ initialize Binance BTCUSDT WebSocket and Prediction REST client
  ├─ Paper: restore simulated balance
  ├─ Live: select Prediction Wallet and query configured USDT payment balance
  └─ restore pending_positions.json
        │
        ▼
Background data
  ├─ Binance BTCUSDT WebSocket ───► rolling spot-price buffer
  └─ Binance Prediction REST ─────► active market, reference price, outcome tokens, books
        │
        ▼
One-second trading tick
  ├─ require warm, fresh BTCUSDT buffer
  ├─ require active Binance BTC five-minute market
  ├─ fetch UP and DOWN Prediction order books concurrently
  ├─ calculate volatility, normalized move, probability, and net edge
  ├─ write every evaluation to observations.csv
  ├─ enforce market/position/risk limits
  └─ Paper fill or Binance MARKET/FOK order
        │
        ▼
Position state
  ├─ append ENTRY to trades.csv after a confirmed fill
  ├─ atomically persist pending_positions.json
  └─ accepted-but-unconfirmed live order → persist + halt new entries
        │
        ▼
Settlement task
  ├─ reconcile any accepted live order through Binance order history
  ├─ Paper: read official Binance market end price
  ├─ Live: read Binance settled-position history and actual PnL
  ├─ append WIN/LOSS and outcomes.csv rows
  └─ Live winner: submit Binance Prediction redeem request
```

## Components

| Component | Responsibility |
| --- | --- |
| `data::binance::BinanceClient` | BTCUSDT ticker WebSocket and reconnect loop |
| `pipeline::price_source::PriceSource` | Rolling spot ticks, trend, and realized volatility |
| `data::binance_prediction::BinancePredictionClient` | Prediction wallet, market discovery, books, quotes, orders, positions, settlement, redeem |
| `MarketState` | Active Binance Prediction topic, token IDs, reference price, expiry |
| `pipeline::decider` | Deterministic Decimal-only probability and risk decisions |
| `pipeline::executor` | Paper fill or model-checked Binance `MARKET/FOK` execution |
| `pipeline::settler` | Atomic persistence of accepted orders and open positions |
| `TradeLog` | Accounting ledger plus model observations/outcome labels |

Shared state is protected by `Arc<RwLock<_>>`; all background work observes one `AtomicBool`
shutdown flag.

## Binance market discovery

The client combines the Binance Prediction market list and BTC semantic search, then verifies full
market detail before accepting a candidate:

- `symbol == BTCUSDT`
- `variantData.type == CRYPTO_UP_DOWN`
- `variantData.priceFeedProvider == BINANCE`
- `variantData.priceFeedSymbol == BTCUSDT`
- duration is within one minute of the configured 300-second interval
- the current time falls in its start/end interval
- one active, unambiguous UP token and one active, unambiguous DOWN token exist

The final condition matters: the bot refuses a market if Binance's response does not make its outcome
mapping unambiguous. It never guesses token direction.

## Decision boundary

`bot.rs` owns I/O. `pipeline::decider` is deterministic and has no network calls:

```text
DecideContext + AccountState + DeciderConfig
  → Pass(reason)
  → Trade(direction, probability, effective entry price, maximum acceptable price, size)
```

The context contains the Prediction market opening/reference price, current Binance spot price,
volatility, 15s/30s spot trends, and executable books. Paper and Live therefore make identical
strategy decisions.

## Order-book and quote boundary

For each token, `fetch_buy_quote`:

1. requests the official Binance Prediction order book;
2. sorts asks from cheapest to most expensive;
3. walks enough levels to spend the fixed USDT amount;
4. calculates weighted effective price, worst required level, selected-side spread, and top-level
   depth;
5. rejects an empty or too-shallow book.

For Live execution the executor requests a Binance `MARKET` quote, verifies its token, wallet,
amount, order type, slippage, expiry, and `minReceive` value against the model maximum, then sends a
`MARKET/FOK` order. The order is not blindly retried.

## Live order reconciliation

A live order acknowledgement is not treated as proof of a fill. The executor queries Binance order
history for actual paid USDT, shares, and fees.

```text
FILLED                → create an open position using actual amounts
REJECTED/CANCELED/... → no position
not visible in time   → persist AwaitingReconciliation and halt all new entries
```

The settlement task keeps reconciling persisted uncertain orders. Restart recovery retains the halt
until that state is resolved, preventing accidental duplicate exposure.

## Settlement and redemption

### Paper

After expiry, the bot re-reads Binance market detail. Once the official `variantData.endPrice` is
available, it compares it with `startPrice`:

```text
endPrice >= startPrice → UP
endPrice <  startPrice → DOWN
```

The Paper payout is filled shares for a winning token and zero otherwise. Simulated market fee uses
Binance's advertised `feeRateBps`.

### Live

The bot reads Binance settled-position history for its exact market topic and token. Binance-reported
`isWinner`, `claimAmount`, and `pnl` are the accounting source of truth. A winning, unredeemed token
is submitted to the Binance batch-redeem endpoint once; a failed recovery can be explicitly retried
with `binance-5m-tools --redeem <token-id>`.

## Persistence and logs

```text
logs/binance/<mode>/
├── bot.log.YYYY-MM-DD
├── trades.csv                # fill and settlement accounting
├── observations.csv          # every model evaluation after warm-up
├── outcomes.csv              # official labels keyed by Prediction market topic
├── balance                   # atomic replacement
├── account_state.json        # persisted risk counters; atomic replacement
├── pending_positions.json    # atomic replacement and restart recovery
└── state_write_failed        # durability marker; present only after a failed state write
```

`account_state.json` persists the risk counters (daily PnL/trade count, loss
streak and last-loss time, and the circuit-breaker window) so that, under working
storage, a crash or restart does not silently reset the daily loss/trade caps or
the loss-streak cooldown. It is written while holding an account lock (which
serializes it against the settlement task), and on entry the pending-position
snapshot is attempted *first*: the successful path writes the position before the
risk counters, prioritizing exposure durability. If either write fails the bot
halts and marks the state suspect (below), so a marker must not be read as proof
that the two files are mutually consistent. Balance
is not trusted from this file: it is re-derived on startup (Paper balance file /
Live payment API). A corrupt or unreadable file fails startup rather than
resetting.

### State-write failure and the durability halt

If any risk/position state write fails, the bot sets a sticky in-memory halt that
blocks all *new* entries (settlement keeps winding down) and writes the
`state_write_failed` marker. The halt is never cleared in-process — this
deliberately avoids a set/clear race between the trade tick and the settlement
task. If the marker is present at startup, entries stay halted from the start,
because the restored snapshot may be stale or incomplete.

The marker does **not** guarantee that an already-filled position or its risk
increments reached disk; it only signals that persisted state is suspect.
Recovery is therefore operator-driven and must not be a bare `rm`:

1. Fix the underlying storage problem.
2. Reconcile `account_state.json` and `pending_positions.json` against
   `trades.csv`, `outcomes.csv`, and Binance order/settled-position history —
   any entries or settlements from the failure window may be missing.
3. Only then delete `logs/binance/<mode>/state_write_failed` and restart.

An automated reconciliation tool is not yet provided; full transactional state
across both files (write-ahead marker before every mutation, ledger replay on
startup) is deferred until this failure class is observed in practice.

## Background tasks

| Task | Default | Responsibility |
| --- | ---: | --- |
| Trading tick | 1 second | Spot state, books, decision, observation, entry |
| Market refresh | 15 seconds | Discover/rotate active Binance BTC five-minute market |
| Settlement | 15 seconds | Reconcile orders, settle positions, redeem winners |
| Status | 10 seconds | Runtime summary |
| BTCUSDT WebSocket | Continuous | Reconnect and ingest Binance spot ticks |
