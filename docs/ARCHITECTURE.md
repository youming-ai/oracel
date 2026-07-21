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
└── pending_positions.json    # atomic replacement and restart recovery
```

## Background tasks

| Task | Default | Responsibility |
| --- | ---: | --- |
| Trading tick | 1 second | Spot state, books, decision, observation, entry |
| Market refresh | 15 seconds | Discover/rotate active Binance BTC five-minute market |
| Settlement | 15 seconds | Reconcile orders, settle positions, redeem winners |
| Status | 10 seconds | Runtime summary |
| BTCUSDT WebSocket | Continuous | Reconnect and ingest Binance spot ticks |
