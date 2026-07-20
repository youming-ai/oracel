# Trading Flow

This document describes the current runtime behavior. Both paper and live mode use the same market
data, decision pipeline, position model, persistence, and settlement logic. Only execution, balance
source, and redemption differ.

## End-to-end flow

```text
Startup
  │
  ├─ load and validate config.toml
  ├─ select logs/paper or logs/live
  ├─ initialize Binance, Gamma, and CLOB clients
  ├─ paper: restore simulated balance
  ├─ live: authenticate CLOB and query on-chain USDC
  └─ restore pending_positions.json
        │
        ▼
Background data
  ├─ Binance BTCUSDT WebSocket ──► rolling price buffer
  └─ Gamma market discovery ─────► current token IDs, slug, condition ID, expiry
        │
        ▼
Trading tick (default: 1 second)
  ├─ require a warm, non-stale BTC buffer
  ├─ require a discovered Polymarket market
  ├─ fetch YES and NO buy quotes concurrently
  ├─ run the decision gates
  ├─ require no existing pending position
  └─ paper simulation or live FAK order
        │
        ▼
Position state
  ├─ deduct actual cost
  ├─ append ENTRY to trades.csv
  └─ atomically persist pending_positions.json
        │
        ▼
Settlement task
  ├─ wait until market expiry
  ├─ poll Gamma until officially closed and resolved
  ├─ calculate payout and PnL
  ├─ append WIN/LOSS to trades.csv
  ├─ remove the pending position and persist state
  └─ live win: attempt on-chain CTF redemption
```

## Startup by mode

### Paper

- No private key or authenticated CLOB client.
- Balance starts from `trading.paper_starting_balance` or `logs/paper/balance`.
- Existing paper positions are restored from `logs/paper/pending_positions.json`.

### Live

- Requires `PRIVATE_KEY`.
- Authenticates with the Polymarket CLOB before entering the run loop.
- Derives the wallet and queries Polygon USDC.
- Initializes the CTF redeemer and restores live pending positions.
- Re-queries available on-chain USDC during trading ticks.

Failure to authenticate, derive the wallet, or obtain the initial live balance aborts startup.

## Data ownership

| Component | Owns |
| --- | --- |
| `PriceSource` | Rolling Binance ticks and BTC trend calculation |
| `MarketState` | Current Polymarket IDs, slug, and expiry |
| `AccountState` | Available balance, PnL, W/L counters, daily PnL |
| `Settler` | The current pending position |
| `BotState` | Idle reason and live FAK retry state |
| `TuiState` | Read-only presentation snapshot and recent rows |

Shared mutable state uses `Arc<RwLock<_>>`. Shutdown uses one shared `AtomicBool` across the bot,
background tasks, and TUI.

## Decision boundary

`bot.rs` owns I/O and orchestration. `pipeline/decider.rs` is deterministic business logic:

```text
DecideContext + AccountState + DeciderConfig → Decision::Pass | Decision::Trade
```

The decider does not perform network calls or place orders. This keeps strategy tests fast and
allows paper/live to share exactly the same decision.

## Execution boundary

`pipeline/executor.rs` converts a trade decision into an `OrderResult`.

- The candidate price receives configured slippage and is rounded for CLOB compatibility.
- The order amount is fixed in USDC rather than rounded up to whole shares.
- Paper simulates a complete fill at the same maximum price.
- Live submits a fixed-USDC FAK buy and records CLOB-reported cost and shares.
- A rejected or zero-fill FAK produces no position.

The main bot permits only one pending position globally. If Gamma resolution is delayed, that
position also blocks entry into later market windows.

## Settlement and redemption

Gamma is the accounting source of truth for both modes. A market is settled only when:

1. `umaResolutionStatus` contains `resolved`;
2. `closed` is true; and
3. one outcome price reaches `misc.resolution_price_threshold`.

For a winning position:

```text
payout = filled_shares
pnl    = payout - actual_cost
```

For a losing position:

```text
payout = 0
pnl    = -actual_cost
```

In live mode, accounting settlement and on-chain redemption are separate steps. A win is queued for
CTF redemption after Gamma resolution. The CLI redemption tools provide recovery for historical or
exhausted automatic retries.

## Persistence

```text
logs/<mode>/balance
logs/<mode>/pending_positions.json
logs/<mode>/trades.csv
```

Balance and pending-position files use write-to-temp plus rename. A corrupt pending-position file
fails startup rather than silently dropping an open position.

## Background tasks

| Task | Default interval | Responsibility |
| --- | ---: | --- |
| Trading tick | 1 second | Quotes, decision, execution, UI snapshot |
| Settlement | 15 seconds | Resolution, PnL, persistence, redemption |
| Market refresh | 60 seconds | Rotate to the active five-minute market |
| Status | 10 seconds | Headless/debug runtime summary |

The Binance WebSocket has its own reconnect loop and receiver task.
