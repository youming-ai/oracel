# Trading Flow

Paper and live mode share market data, probability modeling, risk gates, position persistence, and
settlement. Only order submission, balance source, and redemption differ.

## End-to-end flow

```text
Startup
  ├─ load and validate config.toml
  ├─ select logs/paper or logs/live
  ├─ initialize Binance, Chainlink RTDS, Gamma, and CLOB
  ├─ restore balance and pending_positions.json
  └─ live: authenticate CLOB and initialize Polygon clients
        │
        ▼
Background data
  ├─ Chainlink RTDS ─────────► authoritative BTC/USD buffer
  ├─ Binance WebSocket ──────► low-latency BTCUSDT buffer
  └─ Gamma discovery ────────► market IDs, tokens, slug, and expiry
        │
        ▼
Trading tick
  ├─ require warm and fresh Binance data
  ├─ identify exact Chainlink opening/current values
  ├─ estimate realized volatility and normalized oracle distance
  ├─ fetch both CLOB order books and walk asks for the fixed-USDC amount
  ├─ estimate UP/DOWN probability and apply value, momentum, liquidity, and risk gates
  ├─ append the evaluation to observations.csv
  └─ Paper fill or value-capped live FAK
        │
        ▼
Position state
  ├─ deduct actual cost
  ├─ append ENTRY to trades.csv
  └─ atomically persist pending_positions.json
        │
        ▼
Settlement task
  ├─ poll Gamma after expiry until officially resolved
  ├─ calculate payout from filled shares
  ├─ append WIN/LOSS and persist balance/positions
  └─ live win: attempt on-chain CTF redemption
```

## Data components

| Component | Responsibility |
| --- | --- |
| `data::chainlink::ChainlinkSource` | RTDS reconnect loop, authoritative history, opening price, realized volatility |
| `pipeline::price_source::PriceSource` | Binance buffer and 15s/30s trend calculation |
| `data::polymarket::PolymarketClient` | Full CLOB books and fixed-USDC executable buy quotes |
| `data::market_discovery::MarketDiscovery` | Gamma market rotation and official resolution inference |
| `MarketState` | Current token IDs, condition, slug, and expiry |
| `AccountState` | Balance, PnL, W/L counters, and rolling outcomes |
| `Settler` | Unresolved positions keyed by condition ID |
| `TradeLog` | Accounting ledger plus model-observation dataset |

Shared state uses `Arc<RwLock<_>>`; shutdown uses a shared `AtomicBool`.

## Deterministic decision boundary

`bot.rs` performs network I/O and constructs a context. `pipeline/decider.rs` remains deterministic:

```text
DecideContext + AccountState + DeciderConfig
    -> Decision::Pass(reason)
    -> Decision::Trade {
         direction,
         model_probability,
         entry_price,
         order_limit_price,
         edge,
         size
       }
```

The decider contains Decimal-only probability math and no network calls. Paper and live therefore
make the same decision from the same inputs.

## CLOB quote boundary

For each outcome, `fetch_buy_quote`:

1. fetches the complete order-book summary;
2. sorts asks from cheapest to most expensive;
3. walks enough levels to spend the configured fixed-USDC amount;
4. calculates effective average price and the worst required level;
5. reports selected-side spread, best-ask notional, and book timestamp.

A book without enough total asks returns an error. The decider separately enforces freshness,
selected-side spread, top-level depth, and model value.

## Execution boundary

`pipeline/executor.rs` accepts the model's maximum value-preserving price.

- Paper records a complete fixed-USDC fill at the weighted executable quote.
- Live rounds the model price cap down and submits a fixed-USDC FAK.
- Live accounting uses CLOB-reported `making_amount` and `taking_amount` only.
- Zero fills and rejected FAKs do not create positions.

There is no unconditional slippage multiplier.

## Position concurrency

The condition ID is the uniqueness key, so the same market can never be entered twice. Up to
`strategy.max_unsettled_positions` different conditions may remain unresolved. The default of two
allows a delayed Gamma result without allowing duplicate exposure in one five-minute window.

## Startup by mode

### Paper

- No private key or authenticated CLOB client.
- Restores `logs/paper/balance` and `logs/paper/pending_positions.json`.
- Uses live public market/oracle data and order-book depth.

### Live

- Requires `PRIVATE_KEY` and successful CLOB authentication.
- Queries Polygon USDC and initializes CTF redemption.
- Requires `trading.allow_uncalibrated_model_live = true` while the baseline model remains
  uncalibrated.

Authentication, wallet derivation, or initial balance failure aborts startup.

## Settlement

Gamma remains the accounting source of truth. Resolution requires:

1. `umaResolutionStatus` contains `resolved`;
2. `closed` is true; and
3. one outcome reaches `misc.resolution_price_threshold`.

```text
win:  payout = filled_shares; pnl = payout - actual_cost
loss: payout = 0;             pnl = -actual_cost
```

Live accounting settlement and on-chain redemption are separate. Failed automatic redemption can be
recovered with `polybot-tools`.

## Persistence and logs

```text
logs/<mode>/
├── bot.log.YYYY-MM-DD
├── trades.csv                # entries and accounting settlements
├── observations.csv          # every strategy evaluation after warm-up
├── outcomes.csv              # official outcomes keyed by market slug for calibration
├── balance                   # atomic replacement
└── pending_positions.json    # atomic replacement and restart recovery
```

A corrupt pending-position file fails startup rather than discarding exposure.

## Background tasks

| Task | Default | Responsibility |
| --- | ---: | --- |
| Trading tick | 1s | Books, probability, decision, observation, execution |
| Settlement | 15s | Gamma resolution, PnL, persistence, redemption |
| Market refresh | 60s | Rotate active five-minute market |
| Status | 10s | Runtime summary |
| Binance WebSocket | continuous | Reconnect and ingest exchange ticks |
| Chainlink RTDS | continuous | Reconnect and ingest authoritative ticks/history |
