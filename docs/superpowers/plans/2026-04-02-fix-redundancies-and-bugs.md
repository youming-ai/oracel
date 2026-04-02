# Fix Redundancies & Bugs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 9 identified issues across the trading bot: trades.csv dual-writer race, missing paper-mode logging, decider normalization bug, dead code, duplicate helpers, BalanceState bypass, and phantom Signal stage.

**Architecture:** The fixes are organized into 5 independent tasks ordered by priority. Each task is self-contained — commits compile and test green independently. No cross-task dependencies.

**Tech Stack:** Rust (tokio, rust_decimal, tracing), existing test infrastructure (`cargo test`).

---

## File Structure

| File | Change | Responsibility |
|------|--------|----------------|
| `src/trade_log.rs` | **Create** | Shared `TradeLog` writer — single owner of trades.csv `BufWriter` |
| `src/bot.rs` | **Modify** | Use `TradeLog`; remove duplicate `write_balance`/`decimal` |
| `src/tasks.rs` | **Modify** | Use `TradeLog`; remove duplicate `write_balance` |
| `src/pipeline/decider.rs` | **Modify** | Fix normalization; move `Direction` here; remove duplicate `decimal` |
| `src/pipeline/signal.rs` | **Delete** | Absorbed into decider |
| `src/pipeline/mod.rs` | **Modify** | Remove signal module; add trade_log re-export |
| `src/pipeline/price_source.rs` | **Modify** | Remove `momentum_pct` |
| `src/config.rs` | **Modify** | Remove `BinanceWs`/`CoinbaseWs` variants |
| `src/state.rs` | **Modify** | Remove `BalanceState` debouncer |
| `src/lib.rs` | **Modify** | Add `trade_log` module |
| `src/main.rs` | **Modify** | None (uses re-exports) |

---

### Task 1: Unify trades.csv Writer (P0 + P1)

**Why:** `bot.rs:638-661` (tick) and `tasks.rs:254-269` (settlement checker) write to the same `trades.csv` concurrently with no coordination and different column schemas. Paper mode gets no entry records.

**Files:**
- Create: `src/trade_log.rs`
- Modify: `src/lib.rs`
- Modify: `src/bot.rs`
- Modify: `src/tasks.rs`

- [x] **Step 1: Write the failing test for TradeLog** (pre-existing)

Create test in `src/trade_log.rs`:

```rust
//! Shared trades.csv writer — single owner of the file handle.

use std::io::{BufWriter, Write};
use std::path::Path;

use chrono::Utc;
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct TradeLog {
    writer: Arc<Mutex<BufWriter<std::fs::File>>>,
}

impl TradeLog {
    /// Open (or create) trades.csv at `{log_dir}/trades.csv`.
    /// Writes header if file is new.
    pub fn open(log_dir: &str) -> std::io::Result<Self> {
        let path = Path::new(log_dir).join("trades.csv");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        let metadata = file.metadata()?;
        let mut writer = BufWriter::new(file);
        if metadata.len() == 0 {
            writeln!(
                writer,
                "timestamp,type,direction,order_id,entry_price,cost,edge,balance,remaining_ms,yes_price,no_price,payoff_ratio"
            )?;
        }
        Ok(Self {
            writer: Arc::new(Mutex::new(writer)),
        })
    }

    /// Log a trade entry (called from tick when an order is placed).
    pub async fn log_entry(
        &self,
        direction: &str,
        order_id: &str,
        entry_price: Decimal,
        cost: Decimal,
        edge: Decimal,
        balance: Decimal,
        remaining_ms: i64,
        yes_price: Option<Decimal>,
        no_price: Option<Decimal>,
        payoff_ratio: Decimal,
    ) {
        let line = format!(
            "{},ENTRY,{},{},{:.3},{:.2},{:.1},{:.2},{}s,{:.3},{:.3},{:.1}x\n",
            Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
            direction,
            &order_id[..8.min(order_id.len())],
            entry_price,
            cost,
            edge,
            balance,
            remaining_ms / 1000,
            yes_price.unwrap_or_default(),
            no_price.unwrap_or_default(),
            payoff_ratio,
        );
        let mut w = self.writer.lock().await;
        if let Err(e) = w.write_all(line.as_bytes()) {
            tracing::warn!("[LOG] trades.csv entry write failed: {}", e);
        }
    }

    /// Log a settlement result (called from settlement checker).
    pub async fn log_settlement(
        &self,
        won: bool,
        direction: &str,
        pnl: Decimal,
        entry_btc_price: Decimal,
        current_btc_price: Decimal,
    ) {
        let result = if won { "WIN" } else { "LOSS" };
        let line = format!(
            "{},{},{},{},{:+.2},{:.0},{:.0}\n",
            Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
            result,
            direction,
            "",       // no order_id for settlement
            pnl.round_dp(2),
            entry_btc_price,
            current_btc_price,
        );
        let mut w = self.writer.lock().await;
        if let Err(e) = w.write_all(line.as_bytes()) {
            tracing::warn!("[LOG] trades.csv settlement write failed: {}", e);
        }
    }

    /// Flush buffered writes to disk.
    pub async fn flush(&self) {
        let mut w = self.writer.lock().await;
        if let Err(e) = w.flush() {
            tracing::warn!("[LOG] trades.csv flush failed: {}", e);
        }
    }

    pub fn clone_handle(&self) -> TradeLogHandle {
        TradeLogHandle {
            writer: self.writer.clone(),
        }
    }
}

/// Cheap cloneable handle for passing to background tasks.
#[derive(Clone)]
pub struct TradeLogHandle {
    writer: Arc<Mutex<BufWriter<std::fs::File>>>,
}

impl TradeLogHandle {
    pub async fn log_settlement(
        &self,
        won: bool,
        direction: &str,
        pnl: Decimal,
        entry_btc_price: Decimal,
        current_btc_price: Decimal,
    ) {
        let result = if won { "WIN" } else { "LOSS" };
        let line = format!(
            "{},{},{},{},{:+.2},{:.0},{:.0}\n",
            Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
            result,
            direction,
            "",       // no order_id for settlement
            pnl.round_dp(2),
            entry_btc_price,
            current_btc_price,
        );
        let mut w = self.writer.lock().await;
        if let Err(e) = w.write_all(line.as_bytes()) {
            tracing::warn!("[LOG] trades.csv settlement write failed: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::io::Read;

    fn d(s: &str) -> Decimal {
        Decimal::from_str_exact(s).expect("valid decimal")
    }

    #[test]
    fn test_trade_log_writes_header_on_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let log = TradeLog::open(dir.path().to_str().unwrap()).unwrap();
        // Force flush
        log.writer.blocking_lock().flush().unwrap();

        let content = std::fs::read_to_string(dir.path().join("trades.csv")).unwrap();
        assert!(content.contains("timestamp,type,direction"));
        assert!(content.contains("ENTRY") == false); // header only
    }

    #[test]
    fn test_trade_log_appends_without_header() {
        let dir = tempfile::tempdir().unwrap();
        // Create file with some content
        std::fs::write(dir.path().join("trades.csv"), "existing\n").unwrap();
        let log = TradeLog::open(dir.path().to_str().unwrap()).unwrap();
        log.writer.blocking_lock().flush().unwrap();

        let content = std::fs::read_to_string(dir.path().join("trades.csv")).unwrap();
        assert!(content.starts_with("existing\n"));
        assert!(!content.contains("timestamp")); // no header added
    }

    #[tokio::test]
    async fn test_log_entry_and_settlement_lines() {
        let dir = tempfile::tempdir().unwrap();
        let log = TradeLog::open(dir.path().to_str().unwrap()).unwrap();
        log.log_entry(
            "UP", "abc1234567", d("0.05"), d("5.00"), d("45.0"),
            d("95.00"), 180000, Some(d("0.95")), Some(d("0.05")), d("19.0"),
        ).await;
        log.log_settlement(true, "UP", d("20.0"), d("70000"), d("70500")).await;
        log.flush().await;

        let content = std::fs::read_to_string(dir.path().join("trades.csv")).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        // line 0 = header, line 1 = entry, line 2 = settlement
        assert!(lines[1].contains("ENTRY,UP,abc12345"));
        assert!(lines[2].contains("WIN"));
        assert!(lines[2].contains("UP"));
    }

    #[tokio::test]
    async fn test_handle_logs_settlement() {
        let dir = tempfile::tempdir().unwrap();
        let log = TradeLog::open(dir.path().to_str().unwrap()).unwrap();
        let handle = log.clone_handle();
        handle.log_settlement(false, "DOWN", d("-5.0"), d("70000"), d("69500")).await;
        log.flush().await;

        let content = std::fs::read_to_string(dir.path().join("trades.csv")).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert!(lines[1].contains("LOSS,DOWN"));
    }
}
```

- [x] **Step 2: Run test to verify it compiles and fails/passes** (pre-existing)

Add `tempfile` to dev-dependencies in `Cargo.toml` if not present.

Run: `cargo test --lib trade_log`
Expected: Tests pass (new module with working implementation)

- [x] **Step 3: Add `tempfile` dev-dependency to Cargo.toml** (already present)

In `Cargo.toml`, under `[dev-dependencies]`, add:

```toml
tempfile = "4"
```

Run: `cargo test --lib trade_log`
Expected: All 4 tests pass

- [x] **Step 4: Wire TradeLog into Bot (replace trade_log_writer)**

In `src/lib.rs`, add:

```rust
pub mod trade_log;
```

In `src/bot.rs`:

1. Add import:
```rust
use crate::trade_log::{TradeLog, TradeLogHandle};
```

2. Change the `trade_log_writer` field type in `Bot` struct from `Option<TradeLogWriter>` to `Option<TradeLog>`:

Replace `type TradeLogWriter = Arc<tokio::sync::Mutex<BufWriter<std::fs::File>>>;` — delete this type alias.

3. In `Bot::new()`, replace the `trade_log_writer` initialization block (lines 174-201) with:

```rust
let trade_log = match TradeLog::open(&log_dir) {
    Ok(tl) => {
        tracing::debug!("[INIT] TradeLog opened for {} mode", config.trading.mode);
        Some(tl)
    }
    Err(e) => {
        tracing::warn!("[INIT] Failed to open trade log: {}", e);
        None
    }
};
```

4. In the struct construction, replace `trade_log_writer` with `trade_log`.

5. In `tick()`, replace the entry write block (lines 638-661, inside `if let Some(order) = order`) with:

```rust
if let Some(ref tl) = self.trade_log {
    tl.log_entry(
        order.direction.as_str(),
        &order.order_id,
        order.entry_price,
        order.cost,
        (*edge * decimal("100")).round_dp(1),
        bal,
        remaining_ms,
        poly_yes_dec,
        poly_no_dec,
        payoff_ratio,
    ).await;
}
```

6. Replace the flush_tick handler in `run()` (lines 315-321) with:

```rust
_ = flush_tick.tick() => {
    if let Some(ref tl) = self.trade_log {
        tl.flush().await;
    }
}
```

7. Replace the final flush on shutdown (lines 373-378) with:

```rust
if let Some(ref tl) = self.trade_log {
    tl.flush().await;
}
```

8. Remove unused imports: `BufWriter`, `Write` (from std::io) if no longer used directly.

- [x] **Step 5: Wire TradeLogHandle into tasks.rs**

1. Add import:
```rust
use crate::trade_log::TradeLogHandle;
```

2. Add `trade_log: TradeLogHandle` parameter to `start_settlement_checker`:

```rust
pub(crate) fn start_settlement_checker(
    settler: Arc<RwLock<Settler>>,
    account: Arc<RwLock<AccountState>>,
    price_source: Arc<PriceSource>,
    discovery: Arc<MarketDiscovery>,
    redeemer: Option<Arc<CtfRedeemer>>,
    log_dir: String,
    trade_log: TradeLogHandle,    // NEW
    shutdown: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
```

3. In the settlement checker, replace the direct file write block (lines 242-269, the `tokio::task::spawn_blocking` block) with:

```rust
if let Some(btc_price) = settlement_btc_price {
    for r in &results {
        trade_log.log_settlement(
            r.won,
            r.direction.as_str(),
            r.pnl,
            r.entry_btc_price,
            btc_price,
        ).await;
    }
}
```

4. In `bot.rs:run()`, update the `start_settlement_checker` call to pass the handle:

```rust
let trade_log_handle = self.trade_log.as_ref().map(|tl| tl.clone_handle());
// ... pass trade_log_handle into start_settlement_checker
```

If `trade_log_handle` is `None`, create a no-op handle or adjust the function signature to accept `Option<TradeLogHandle>`.

**Recommended approach:** Make the parameter `Option<TradeLogHandle>`:

```rust
pub(crate) fn start_settlement_checker(
    // ... existing params ...
    trade_log: Option<TradeLogHandle>,
    shutdown: Arc<AtomicBool>,
)
```

And guard the settlement logging:

```rust
if let (Some(ref tl), Some(btc_price)) = (&trade_log, settlement_btc_price) {
    for r in &results {
        tl.log_settlement(r.won, r.direction.as_str(), r.pnl, r.entry_btc_price, btc_price).await;
    }
}
```

5. In `bot.rs:run()`, pass the handle:

```rust
let trade_log_handle = self.trade_log.as_ref().map(|tl| tl.clone_handle());
let mut settlement_handle = tasks::start_settlement_checker(
    self.settler.clone(),
    self.account.clone(),
    self.price_source.clone(),
    self.discovery.clone(),
    self.redeemer.clone(),
    self.log_dir.clone(),
    trade_log_handle,
    self.shutdown.clone(),
);
```

- [x] **Step 6: Run full test suite**

Run: `cargo test --locked`
Expected: All existing tests pass + new trade_log tests pass

- [x] **Step 7: Commit**

```bash
git add src/trade_log.rs src/lib.rs src/bot.rs src/tasks.rs Cargo.toml
git commit -m "fix: unify trades.csv writer — single TradeLog owner, all modes

- Eliminates dual-writer race condition between tick() and settlement checker
- Paper mode now logs entry records (previously only settlements)
- Adds type column (ENTRY/SETTLEMENT) for disambiguation
- Shared Arc<Mutex<BufWriter>> via TradeLog + TradeLogHandle
- Adds tempfile dev-dep for tests"
```

---

### Task 2: Fix Decider Price Normalization (P1)

**Why:** `decider.rs:150-166` normalizes `yes/(yes+no)` and `no/(yes+no)`, but Polymarket mid-prices are already probabilities in [0,1]. When `yes+no ≠ 1.0`, normalization produces incorrect values. For example, `yes=0.95, no=0.06` gives `mkt_up=0.94` instead of the raw `0.95`.

**Files:**
- Modify: `src/pipeline/decider.rs`

- [x] **Step 1: Write tests demonstrating the normalization issue**

In `src/pipeline/decider.rs` tests, add:

```rust
#[test]
fn test_uses_raw_yes_price_not_normalized() {
    // yes=0.95, no=0.06 → total=1.01 → normalized mkt_up=0.941 (WRONG)
    // Raw yes=0.95 > 0.90 → should trigger Down trade
    let account = AccountState::new(d("1000"));
    let cfg = DeciderConfig {
        max_entry_price: d("0.50"),
        ..DeciderConfig::default()
    };
    let ctx = DecideContext {
        market_yes: Some(d("0.95")),
        market_no: Some(d("0.06")),
        remaining_ms: 240_000,
    };

    let decision = decide(&ctx, &account, &cfg);
    match decision {
        Decision::Trade { direction, edge, .. } => {
            assert_eq!(direction, Direction::Down);
            // cheap side = no = 0.06, edge = 0.50 - 0.06 = 0.44
            assert_eq!(edge, d("0.44"));
        }
        Decision::Pass(reason) => panic!("expected trade but got: {}", reason),
    }
}

#[test]
fn test_extreme_bearish_uses_raw_no_price() {
    // yes=0.03, no=0.96 → should trigger Up trade (raw no=0.96 > 0.90)
    let account = AccountState::new(d("1000"));
    let cfg = DeciderConfig {
        max_entry_price: d("0.50"),
        ..DeciderConfig::default()
    };
    let ctx = DecideContext {
        market_yes: Some(d("0.03")),
        market_no: Some(d("0.96")),
        remaining_ms: 240_000,
    };

    let decision = decide(&ctx, &account, &cfg);
    match decision {
        Decision::Trade { direction, edge, .. } => {
            assert_eq!(direction, Direction::Up);
            // cheap side = yes = 0.03, edge = 0.50 - 0.03 = 0.47
            assert_eq!(edge, d("0.47"));
        }
        Decision::Pass(reason) => panic!("expected trade but got: {}", reason),
    }
}
```

Run: `cargo test --lib decider::tests::test_uses_raw_yes_price`
Expected: FAIL — current code normalizes and gets wrong values

- [x] **Step 2: Fix decide() to use raw prices**

In `decider.rs`, replace lines 150-166 with:

```rust
    // Use raw Polymarket mid-prices directly (already probabilities in [0,1]).
    // Normalization by (yes+no) is removed — it distorts values when yes+no≠1.0.
    if yes > cfg.extreme_threshold {
        // Market is extremely bullish → bet against (Down)
        (Direction::Down, no)
    } else if no > cfg.extreme_threshold {
        // Market is extremely bearish → bet against (Up)
        (Direction::Up, yes)
    } else {
        return Decision::Pass(format!(
            "not_extreme_{}%",
            (yes * decimal("100")).round_dp(0)
        ));
    };
```

Key changes:
- Compare `yes` directly against threshold (was `yes/total`)
- Compare `no` directly against threshold (was `(1-yes/total)`)
- Use raw `yes`/`no` as `cheap_price` (was `no/total` or `yes/total`)

- [x] **Step 3: Run all tests**

Run: `cargo test --lib`
Expected: All tests pass, including new normalization tests

- [x] **Step 4: Update existing tests that depended on normalization**

The existing test `test_extreme_bullish_allows_trade` uses `yes=0.97, no=0.03` with `threshold=0.90`. After the fix:
- `yes=0.97 > 0.90` → Direction::Down, cheap_price=no=0.03
- edge = 0.50 - 0.03 = 0.47

This matches existing expectations. Verify by running:

Run: `cargo test --lib decider`
Expected: All pass — the test values happen to work with both approaches since yes+no=1.0

- [x] **Step 5: Commit**

```bash
git add src/pipeline/decider.rs
git commit -m "fix: use raw mid-prices in decider instead of normalizing by total

Polymarket mid-prices are already probabilities. Normalizing by (yes+no)
produced incorrect thresholds when the sum deviated from 1.0 (e.g., due to
spread). Now compares yes/no directly against extreme_threshold."
```

---

### Task 3: Remove Dead Code + Consolidate Helpers (P3)

**Why:** `momentum_pct` is never called. `write_balance` is duplicated in `bot.rs` and `tasks.rs`. `decimal()` helper is duplicated in `bot.rs` and `decider.rs`.

**Files:**
- Modify: `src/pipeline/price_source.rs`
- Create: `src/util.rs`
- Modify: `src/lib.rs`
- Modify: `src/bot.rs`
- Modify: `src/tasks.rs`
- Modify: `src/pipeline/decider.rs`

- [x] **Step 1: Create shared utility module**

Create `src/util.rs`:

```rust
//! Shared helpers.

use rust_decimal::Decimal;
use std::path::Path;

pub(crate) fn decimal(value: &'static str) -> Decimal {
    Decimal::from_str_exact(value).expect(value)
}

/// Atomically write balance to file (write tmp, then rename).
pub(crate) async fn write_balance(log_dir: &str, bal: Decimal) {
    let tmp = Path::new(log_dir).join("balance.tmp");
    let dst = Path::new(log_dir).join("balance");
    let text = format!("{}", bal.normalize());
    if let Err(e) = tokio::fs::write(&tmp, &text).await {
        tracing::warn!("[STATE] Failed to write balance: {}", e);
        return;
    }
    if let Err(e) = tokio::fs::rename(&tmp, &dst).await {
        tracing::warn!("[STATE] Failed to rename balance file: {}", e);
    }
}
```

- [x] **Step 2: Update lib.rs**

In `src/lib.rs`, add:

```rust
pub mod util;
```

- [x] **Step 3: Remove momentum_pct from PriceSource**

In `src/pipeline/price_source.rs`, delete lines 104-121 (the `momentum_pct` method).

Run: `cargo build`
Expected: Compiles — if `momentum_pct` is truly unused, no errors

- [x] **Step 4: Replace write_balance in bot.rs**

1. Remove the `write_balance` method from `Bot` impl (lines 228-240).
2. Remove the `decimal` function from bot.rs (lines 33-35).
3. Add import: `use crate::util;`
4. Replace all `Self::write_balance(...)` calls with `util::write_balance(...)`.
5. Replace all `decimal("...")` calls with `util::decimal("...")`.

- [x] **Step 5: Replace write_balance in tasks.rs**

1. Remove the `write_balance` function from tasks.rs (lines 23-35).
2. Add import: `use crate::util;`
3. Replace all `write_balance(...)` calls with `util::write_balance(...)`.

- [x] **Step 6: Replace decimal helper in decider.rs**

1. Remove the `decimal` function from `src/pipeline/decider.rs` (lines 49-51).
2. Add import: `use crate::util;`
3. Replace all `decimal("...")` calls with `util::decimal("...")`.

- [x] **Step 7: Run full CI check**

Run: `cargo build --locked && cargo test --locked && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo fmt --all -- --check`
Expected: All pass

- [x] **Step 8: Commit**

```bash
git add src/util.rs src/lib.rs src/pipeline/price_source.rs src/bot.rs src/tasks.rs src/pipeline/decider.rs
git commit -m "refactor: consolidate shared helpers, remove dead code

- Extract write_balance() and decimal() to util module
- Remove duplicate write_balance from bot.rs and tasks.rs
- Remove duplicate decimal() from bot.rs and decider.rs
- Remove unused momentum_pct() from PriceSource"
```

---

### Task 4: Remove BalanceState Debouncer (P2)

**Why:** `state.rs:9-67` defines `BalanceState` with debounced writes ($1 threshold, 60s interval), but `bot.rs` bypasses it with direct `write_balance` calls after trades. The debouncer only guards the on-chain sync path — inconsistent and misleading.

**Files:**
- Modify: `src/state.rs`
- Modify: `src/bot.rs`

- [x] **Step 1: Remove BalanceState from state.rs**

In `src/state.rs`:

1. Remove the `BalanceState` struct and all its methods/impl blocks (lines 8-73).
2. Remove the `balance_state` field from `BotState`:
```rust
pub(crate) struct BotState {
    pub last_no_trade_reason: String,
    pub last_idle_reason: String,
    pub fak_rejections: u32,
    pub fak_market_ms: i64,
    pub last_fak_rejection_ms: i64,
}
```
3. Remove `balance_state: BalanceState::new()` from `BotState::new()`.

4. Remove the `AtomicU64` and `Arc` imports (if no longer used).

- [x] **Step 2: Remove BalanceState usage from bot.rs tick()**

In `bot.rs:tick()`, remove the debounced-write block (lines 396-406):

```rust
// DELETE this block:
let should_write = {
    let state = self.state.read().await;
    state.balance_state.should_write(on_chain_bal)
};

if should_write {
    Self::write_balance(&self.log_dir, on_chain_bal).await;
    let state = self.state.read().await;
    state.balance_state.record_write(on_chain_bal);
    tracing::debug!("[BALANCE] Wrote balance: ${:.2}", on_chain_bal);
}
```

Replace with a simpler debounce — just check if balance changed:

```rust
{
    let mut acc = self.account.write().await;
    if acc.balance != on_chain_bal {
        tracing::debug!("[BALANCE] On-chain balance updated: ${:.2}", on_chain_bal);
        acc.balance = on_chain_bal;
        drop(acc);
        util::write_balance(&self.log_dir, on_chain_bal).await;
    }
}
```

Remove the early `account.balance = on_chain_bal` write and lock on line 391-393 since it's now inside the if-block.

- [x] **Step 3: Run tests**

Run: `cargo test --locked && cargo clippy --workspace --all-targets --all-features -- -D warnings`
Expected: All pass

- [x] **Step 4: Commit**

```bash
git add src/state.rs src/bot.rs
git commit -m "refactor: remove BalanceState debouncer, simplify on-chain sync

BalanceState was only used for one write path while others bypassed it.
Replaced with a simple balance-changed check in tick()."
```

---

### Task 5: Merge Signal into Decider (P2)

**Why:** `signal.rs` only defines the `Direction` enum — no computation. The CLAUDE.md "5-stage pipeline" is misleading since there's no SignalComputer. Moving `Direction` to `decider.rs` and removing `signal.rs` eliminates the phantom stage.

**Files:**
- Modify: `src/pipeline/signal.rs` → delete file
- Modify: `src/pipeline/decider.rs` — absorb Direction
- Modify: `src/pipeline/executor.rs`
- Modify: `src/pipeline/settler.rs`
- Modify: `src/pipeline/mod.rs`
- Modify: `src/bot.rs`
- Modify: `src/tasks.rs`
- Modify: `src/data/market_discovery.rs`

- [x] **Step 1: Move Direction to decider.rs**

In `src/pipeline/decider.rs`, add at the top (before the `Decision` enum):

```rust
/// Trade direction — which outcome the bot bets on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Direction {
    Up,
    Down,
}

impl Direction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Direction::Up => "UP",
            Direction::Down => "DOWN",
        }
    }
}
```

Change the import in decider.rs from:
```rust
use crate::pipeline::signal::Direction;
```
to nothing (Direction is now local).

- [x] **Step 2: Update all imports across codebase**

Every file that imports `crate::pipeline::signal::Direction` must change to `crate::pipeline::decider::Direction`.

Files to update:

**`src/pipeline/executor.rs`:**
```rust
// was: use crate::pipeline::signal::Direction;
use crate::pipeline::decider::Direction;
```

**`src/pipeline/settler.rs`:**
```rust
// was: use crate::pipeline::signal::Direction;
use crate::pipeline::decider::Direction;
```

**`src/bot.rs`:**
```rust
// was: use pipeline::signal::Direction;
use pipeline::decider::Direction;
```
(Remove the old `use pipeline::signal::Direction` line.)

**`src/data/market_discovery.rs`:**
```rust
// was: use crate::pipeline::signal::Direction;
use crate::pipeline::decider::Direction;
```

**`src/tasks.rs`:** — does not directly import Direction (uses it through Settler types), no change needed.

- [x] **Step 3: Update pipeline/mod.rs**

Remove the signal module:
```rust
pub mod decider;
pub mod executor;
pub mod price_source;
pub mod settler;
// REMOVED: pub mod signal;
```

- [x] **Step 4: Delete signal.rs**

```bash
rm src/pipeline/signal.rs
```

- [x] **Step 5: Run full CI check**

Run: `cargo build --locked && cargo test --locked && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo fmt --all -- --check`
Expected: All pass

- [x] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor: merge Direction enum from signal.rs into decider.rs

signal.rs only contained the Direction enum with no logic. Moving it to
decider.rs where direction is actually determined eliminates the phantom
pipeline stage."
```

---

## Self-Review Checklist

### Spec Coverage
| Issue | Task | Status |
|-------|------|--------|
| P0: trades.csv dual-writer race | Task 1 | Covered |
| P1: Paper mode no entry logs | Task 1 | Covered (TradeLog opens for all modes) |
| P1: Decider normalization bug | Task 2 | Covered |
| P3: Dead code (momentum_pct) | Task 3 | Covered |
| P3: Duplicate write_balance | Task 3 | Covered |
| P3: Duplicate decimal() | Task 3 | Covered |
| P2: BalanceState bypass | Task 4 | Covered |
| P2: Signal phantom stage | Task 5 | Covered |
| P3: PriceSourceType Ws variants | Not included — low impact, API breaking | Deferred |

### Placeholder Scan
No TBD/TODO/fill-in-later patterns found. All code blocks contain complete implementations.

### Type Consistency
- `TradeLog` / `TradeLogHandle` defined in Task 1, used consistently in Tasks 1
- `Direction` defined in Task 5 decider.rs, re-exported path matches all import updates
- `util::decimal()` and `util::write_balance()` defined in Task 3, replaces all local copies
- `config.json` `PriceSourceType` variants deliberately left unchanged (deferred)

### Dashboard Compatibility
The `TradeLog` adds a `type` column (ENTRY/WIN/LOSS) at position 2. This changes the CSV schema. The dashboard CSV parser (`dashboard/src/lib/csv-parser.ts`) must be updated to handle the new column. This is noted but **not included in this plan** — it should be a separate follow-up task.

**Important:** The settlement line format changes from:
```
timestamp,WIN,UP,,+20.00,70000,70500
```
The dashboard parser currently checks `cols[1]` for WIN/LOSS. After this change, WIN/LOSS moves to `cols[1]` (still correct — the `type` column IS the result for settlements). Entry records now have `ENTRY` at `cols[1]`. The parser must be updated to distinguish ENTRY rows.
