# Paper/Live Data Separation & Position Sizing Simplification

## Problem

All runtime data (`bot.log`, `trades.csv`, `balance`, `state.json`) is written to a single `logs/` directory regardless of trading mode. Paper and live data are mixed together, making it impossible to distinguish which mode produced which data.

Additionally, position sizing uses a complex Half-Kelly calculation with five config parameters. This should be replaced with a simple fixed-fraction approach.

## Decisions

- **Log directory**: `logs/paper/` and `logs/live/` based on `TradingMode`
- **Paper balance**: Default 100 USDC on first run; persisted in `logs/paper/balance` for continuity
- **Live balance**: Queried from wallet on-chain (USDC `balanceOf`) before every trade; `logs/live/balance` written for monitoring only
- **Position sizing**: `balance / 100`, minimum 1 USDC. Replaces Half-Kelly, `max_position_size`, `min_order_size`, `max_risk_fraction`
- **State isolation**: Each mode has independent `state.json`, so paper losses don't affect live circuit breakers
- **Historical data**: Existing `logs/` root files will be deleted (user chose clean start)

## Design

### 1. Log Directory Routing

**Current**: `const LOG_DIR: &str = "logs"` in `main.rs` (11 references).

**New**: A `log_dir` field on `Bot`, computed at startup from `TradingMode`:

```rust
fn log_dir(mode: TradingMode) -> String {
    format!("logs/{}", mode) // TradingMode already implements Display as "paper"/"live"
}
```

All `Path::new(LOG_DIR).join(...)` calls become `Path::new(&self.log_dir).join(...)`. The `LOG_DIR` constant is removed.

The tracing appender setup in `main()` also uses this computed path so that `bot.log` lands in the correct subdirectory.

**Directory structure**:

```
logs/
  paper/
    bot.log
    trades.csv
    balance
    state.json
  live/
    bot.log
    trades.csv
    balance
    state.json
    redeem.log
```

### 2. Balance Initialization

#### Paper Mode

- On startup, read `logs/paper/balance`. If present, use it (supports session continuity).
- If absent (first run or after cleanup), initialize to 100 USDC.
- Add `const PAPER_INITIAL_BALANCE: Decimal = dec("100")` in an appropriate location (not in `config.json`).
- Balance updates continue to be tracked locally via PnL and written to `logs/paper/balance`.

#### Live Mode

- Before each trade decision, query the wallet's USDC balance on-chain.
- New function in `polymarket.rs`:

```rust
pub(crate) async fn query_usdc_balance(rpc_url: &str, wallet: Address) -> Result<Decimal> {
    // ERC20 balanceOf(wallet) on POLYGON_USDC
    // USDC on Polygon has 6 decimals: divide raw value by 10^6
}
```

- Uses the existing `POLYGON_USDC` address constant and the same RPC URL selection logic as `CtfRedeemer` (Alchemy if available, else public Polygon RPC).
- The queried balance replaces the locally-tracked balance in `AccountState` before each decision cycle.
- `logs/live/balance` is still written (for monitoring/`watch.sh`) but is never read as the source of truth.

### 3. Position Sizing

**Current** (decider.rs lines 326-344):

```
Half-Kelly fraction * edge multiplier * balance
  -> clamp(min_position, max_position)
  -> cap at balance * max_risk_fraction
```

**New**:

```rust
let size = (account.balance / dec("100")).max(dec("1"));
```

Fixed 1% of balance, floor at 1 USDC. No edge scaling, no Kelly, no win-rate tracking for sizing purposes.

**Config fields removed**:

| Location | Field | Reason |
|---|---|---|
| `StrategyConfig` | `max_position_size` | Replaced by balance/100 |
| `StrategyConfig` | `min_order_size` | Replaced by 1 USDC floor |
| `RiskConfig` | `max_risk_fraction` | Replaced by fixed 1% |
| `DeciderConfig` | `max_position` | Same |
| `DeciderConfig` | `min_position` | Same |
| `DeciderConfig` | `max_risk_fraction` | Same |

**Config fields preserved** (unchanged):

- `edge_threshold` (trade entry gate, not sizing)
- `max_consecutive_losses`, `max_daily_loss_pct`, `cooldown_ms` (risk/circuit breaker logic)
- `extreme_threshold`, `fair_value`, `momentum_*` (signal logic)

**`config.json` and `config.example.json`**: Remove `max_position_size`, `min_order_size`, `max_risk_fraction`.

**`Config::validate()`**: Remove checks for the three deleted fields.

**`Config::is_default_non_trading()`**: Remove comparisons for the three deleted fields.

## Files Changed

| File | Changes |
|---|---|
| `src/main.rs` | Remove `LOG_DIR` const; add `log_dir` field to `Bot`; update all path references; paper balance default 100; live balance refresh from chain before each cycle |
| `src/config.rs` | Remove `max_position_size`, `min_order_size` from `StrategyConfig`; remove `max_risk_fraction` from `RiskConfig`; update defaults, validation, `is_default_non_trading` |
| `src/pipeline/decider.rs` | Replace Half-Kelly sizing block with `balance/100` (min 1); remove `max_position`, `min_position`, `max_risk_fraction` from `DeciderConfig`; update tests |
| `src/data/polymarket.rs` | Add `query_usdc_balance()` function using existing `POLYGON_USDC` address |
| `config.json` | Remove three fields |
| `config.example.json` | Remove three fields |

## Out of Scope

- Simultaneous paper+live execution (single process, single mode)
- Migration of existing log files (user chose to delete and start fresh)
- Changes to signal logic, edge thresholds, or risk circuit breakers
- Changes to the `redeem` CLI commands
