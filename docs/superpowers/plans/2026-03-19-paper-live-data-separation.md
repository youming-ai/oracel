# Paper/Live Data Separation Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Separate paper/live data into `logs/paper/` and `logs/live/`, simplify position sizing to balance/100, and fetch live balance from wallet on-chain.

**Architecture:** Replace the hardcoded `LOG_DIR` constant with a mode-aware path computed at startup. Remove Half-Kelly sizing and three config fields in favor of fixed 1% position sizing. Add ERC20 `balanceOf` query for live mode wallet balance.

**Tech Stack:** Rust, alloy (Polygon RPC), rust_decimal, tracing-appender

**Spec:** `docs/superpowers/specs/2026-03-19-paper-live-data-separation-design.md`

---

## File Structure

| File | Role | Action |
|---|---|---|
| `src/config.rs` | Config structs and validation | Modify: remove 3 fields |
| `src/pipeline/decider.rs` | Trade decision + sizing | Modify: simplify sizing |
| `src/main.rs` | Bot struct, logging, balance | Modify: log_dir routing, balance init |
| `src/data/polymarket.rs` | Polymarket + on-chain interaction | Modify: add USDC balance query |
| `config.json` | Runtime config | Modify: remove 3 fields |
| `config.example.json` | Example config | Modify: remove 3 fields |
| `scripts/watch.sh` | Terminal monitor | Modify: mode-aware paths |

---

## Chunk 1: Config Cleanup + Position Sizing

### Task 1: Remove config fields

**Files:**
- Modify: `src/config.rs`
- Modify: `config.json`
- Modify: `config.example.json`

- [ ] **Step 1: Remove fields from `StrategyConfig`**

In `src/config.rs`, remove `max_position_size` and `min_order_size` from the `StrategyConfig` struct (lines 84-88):

```rust
// REMOVE these two fields from StrategyConfig:
//   #[serde(with = "rust_decimal::serde::float")]
//   pub max_position_size: Decimal,
//   #[serde(with = "rust_decimal::serde::float")]
//   pub min_order_size: Decimal,
```

Remove them from `Default for StrategyConfig` (lines 196-198):

```rust
// REMOVE from Default impl:
//   max_position_size: dec("50.0"),
//   min_order_size: dec("5.0"),
```

- [ ] **Step 2: Remove `max_risk_fraction` from `RiskConfig`**

In `src/config.rs`, remove from `RiskConfig` struct (lines 143-147):

```rust
// REMOVE from RiskConfig:
//   #[serde(
//       default = "default_max_risk_fraction",
//       with = "rust_decimal::serde::float"
//   )]
//   pub max_risk_fraction: Decimal,
```

Remove from `Default for RiskConfig` (line 222):

```rust
// REMOVE: max_risk_fraction: dec("0.10"),
```

Remove the `default_max_risk_fraction` function (lines 156-158):

```rust
// REMOVE:
// fn default_max_risk_fraction() -> Decimal {
//     dec("0.10")
// }
```

- [ ] **Step 3: Update `Config::validate()`**

Remove these checks from `validate()`:

```rust
// REMOVE lines 277-284:
// if self.strategy.max_position_size <= zero {
//     anyhow::bail!("strategy.max_position_size must be > 0");
// }
// if self.strategy.min_order_size <= zero {
//     anyhow::bail!("strategy.min_order_size must be > 0");
// }
// if self.strategy.min_order_size > self.strategy.max_position_size {
//     anyhow::bail!("strategy.min_order_size must be <= strategy.max_position_size");
// }

// REMOVE lines 292-294:
// if !(zero < self.risk.max_risk_fraction && self.risk.max_risk_fraction <= one) {
//     anyhow::bail!("risk.max_risk_fraction must be in (0, 1]");
// }
```

- [ ] **Step 4: Update `Config::is_default_non_trading()`**

Remove these comparisons:

```rust
// REMOVE from is_default_non_trading():
// && self.strategy.max_position_size == defaults.strategy.max_position_size
// && self.strategy.min_order_size == defaults.strategy.min_order_size
// ... (inside the chain)
// && self.risk.max_risk_fraction == defaults.risk.max_risk_fraction
```

- [ ] **Step 5: Update config tests**

Remove `test_validate_rejects_min_greater_than_max` test entirely (lines 340-346) since the fields no longer exist.

- [ ] **Step 6: Update JSON config files**

`config.json` — remove 3 fields:

```json
{
  "trading": {
    "mode": "paper"
  },
  "market": {
    "window_minutes": 5.0
  },
  "polyclob": {
    "gamma_api_url": "https://gamma-api.polymarket.com"
  },
  "strategy": {
    "extreme_threshold": 0.8,
    "fair_value": 0.5,
    "btc_tiebreaker_usd": 5.0,
    "momentum_threshold": 0.001,
    "momentum_lookback_ms": 120000
  },
  "edge": {
    "edge_threshold_early": 0.15
  },
  "risk": {
    "max_consecutive_losses": 5,
    "max_daily_loss_pct": 0.15,
    "cooldown_ms": 5000
  },
  "polling": {
    "signal_interval_ms": 1000
  }
}
```

`config.example.json` — same structure (remove `max_position_size`, `min_order_size`, `max_risk_fraction`).

- [ ] **Step 7: Verify removal is complete**

Do NOT commit yet — `main.rs` and `decider.rs` still reference the removed fields. Task 2 will fix those and commit everything together to keep the codebase compilable at every commit.

---

### Task 2: Simplify position sizing in decider

**Files:**
- Modify: `src/pipeline/decider.rs`
- Modify: `src/main.rs:504-516` (DeciderConfig construction)

- [ ] **Step 1: Write failing test for new sizing logic**

Add this test in `decider.rs` `mod tests`:

```rust
#[test]
fn test_position_size_is_one_percent_of_balance() {
    let mut account = AccountState::new(d("500"));
    account.last_trade_time_ms = chrono::Utc::now().timestamp_millis() - 60_000;

    let cfg = DeciderConfig::default();
    let decision = decide(
        Some(d("0.85")),
        Some(d("0.15")),
        1_700_000_000_000,
        &account,
        &cfg,
        &[], // no momentum data = no momentum filter
    );

    match decision {
        Decision::Trade { size_usdc, .. } => {
            assert_eq!(size_usdc, d("5")); // 500 / 100 = 5
        }
        Decision::Pass(reason) => panic!("expected trade but got pass: {}", reason),
    }
}

#[test]
fn test_position_size_floor_at_one_usdc() {
    let mut account = AccountState::new(d("50"));
    account.last_trade_time_ms = chrono::Utc::now().timestamp_millis() - 60_000;

    let cfg = DeciderConfig::default();
    let decision = decide(
        Some(d("0.85")),
        Some(d("0.15")),
        1_700_000_000_000,
        &account,
        &cfg,
        &[],
    );

    match decision {
        Decision::Trade { size_usdc, .. } => {
            assert_eq!(size_usdc, d("1")); // 50 / 100 = 0.5, floored to 1
        }
        Decision::Pass(reason) => panic!("expected trade but got pass: {}", reason),
    }
}
```

- [ ] **Step 2: Remove 3 fields from `DeciderConfig`**

Remove `max_position`, `min_position`, `max_risk_fraction` from the struct and its `Default` impl:

```rust
#[derive(Debug, Clone)]
pub(crate) struct DeciderConfig {
    /// Minimum edge to trade (15%)
    pub edge_threshold: Decimal,
    /// Cooldown between trades (ms)
    pub cooldown_ms: i64,
    /// Market price threshold to consider "extreme" (e.g. 0.80)
    pub extreme_threshold: Decimal,
    /// Fair value assumption for binary outcome (e.g. 0.50)
    pub fair_value: Decimal,
    /// Maximum consecutive losses before circuit breaker
    pub max_consecutive_losses: u32,
    /// Maximum daily loss as fraction of balance (e.g. 0.10 = 10%)
    pub max_daily_loss_pct: Decimal,
    /// BTC momentum threshold to skip trade (e.g. 0.001 = 0.1%)
    pub momentum_threshold: Decimal,
    /// Momentum lookback window in milliseconds (e.g. 120_000 = 2 min)
    pub momentum_lookback_ms: i64,
}

impl Default for DeciderConfig {
    fn default() -> Self {
        Self {
            edge_threshold: decimal("0.15"),
            cooldown_ms: 5_000,
            extreme_threshold: decimal("0.80"),
            fair_value: decimal("0.50"),
            max_consecutive_losses: 8,
            max_daily_loss_pct: decimal("0.10"),
            momentum_threshold: decimal("0.001"),
            momentum_lookback_ms: 120_000,
        }
    }
}
```

- [ ] **Step 3: Replace sizing logic in `decide()`**

Replace lines 326-344 (the entire Half-Kelly block) with:

```rust
    // 6. Position sizing: fixed 1% of balance, floor at 1 USDC
    let size = (account.balance / decimal("100")).max(decimal("1"));
```

Also remove the now-unused `AccountState::overall_win_rate()` method (lines 221-229) — it was only used for Kelly sizing.

- [ ] **Step 4: Update DeciderConfig construction in `main.rs`**

Replace lines 504-516 in `main.rs`:

```rust
        let decider_cfg = DeciderConfig {
            edge_threshold: self.config.edge.edge_threshold_early,
            cooldown_ms: self.config.risk.cooldown_ms,
            extreme_threshold: self.config.strategy.extreme_threshold,
            fair_value: self.config.strategy.fair_value,
            max_consecutive_losses: self.config.risk.max_consecutive_losses,
            max_daily_loss_pct: self.config.risk.max_daily_loss_pct,
            momentum_threshold: self.config.strategy.momentum_threshold,
            momentum_lookback_ms: self.config.strategy.momentum_lookback_ms,
        };
```

- [ ] **Step 5: Update test helper `cfg_for_threshold_test()`**

Remove the 3 deleted fields from the test helper in `decider.rs`:

```rust
fn cfg_for_threshold_test() -> DeciderConfig {
    DeciderConfig {
        edge_threshold: d("0.15"),
        cooldown_ms: 5_000,
        extreme_threshold: d("0.64"),
        fair_value: d("0.50"),
        max_consecutive_losses: 8,
        max_daily_loss_pct: d("0.10"),
        momentum_threshold: d("0.001"),
        momentum_lookback_ms: 120_000,
    }
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test 2>&1`
Expected: All tests pass, including the two new sizing tests.

- [ ] **Step 7: Commit config cleanup + sizing together**

Both Task 1 and Task 2 changes are committed as one atomic unit so the codebase compiles at every commit:

```bash
git add src/config.rs config.json config.example.json src/pipeline/decider.rs src/main.rs
git commit -m "refactor: remove Half-Kelly sizing, simplify to fixed 1% position (balance/100, min 1 USDC)"
```

---

## Chunk 2: Log Directory Routing + Balance

### Task 3: Route logs to mode-specific subdirectories

**Files:**
- Modify: `src/main.rs`

This is the largest single change. The `LOG_DIR` constant is removed, and all 11 references are updated to use a `log_dir: String` field threaded through the code.

- [ ] **Step 1: Add `log_dir` field to `Bot` and update constructor**

Add field to `Bot` struct:

```rust
struct Bot {
    config: Config,
    log_dir: String, // NEW
    price_source: Arc<PriceSource>,
    // ... rest unchanged
}
```

Add `log_dir` parameter to `Bot::new()`:

```rust
async fn new(config: Config, log_dir: String) -> Result<Self> {
```

Set it in the return value:

```rust
Ok(Self {
    config,
    log_dir, // NEW
    price_source,
    // ... rest unchanged
})
```

- [ ] **Step 2: Change static methods to take `log_dir` parameter**

Remove `const LOG_DIR: &str = "logs";` (line 23).

Update `load_balance`:

```rust
async fn load_balance(log_dir: &str) -> Option<Decimal> {
    let content = tokio::fs::read_to_string(Path::new(log_dir).join("balance"))
        .await
        .ok()?;
    content.trim().parse().ok()
}
```

Update `write_balance`:

```rust
async fn write_balance(log_dir: &str, bal: Decimal) {
    let tmp = Path::new(log_dir).join("balance.tmp");
    let dst = Path::new(log_dir).join("balance");
    let text = format!("{:.2}", bal);
    let _ = tokio::fs::write(&tmp, &text).await;
    let _ = tokio::fs::rename(&tmp, &dst).await;
}
```

Update `load_state`:

```rust
async fn load_state(log_dir: &str) -> PersistState {
    let path = Path::new(log_dir).join("state.json");
    match tokio::fs::read_to_string(&path).await {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => PersistState::default(),
    }
}
```

Update `save_state`:

```rust
async fn save_state(log_dir: &str, settler: &Arc<RwLock<Settler>>, account: &Arc<RwLock<AccountState>>) {
    let positions = settler.read().await.pending_positions();
    let acc = account.read().await;
    let state = PersistState {
        pending_positions: positions,
        last_traded_settlement_ms: acc.last_traded_settlement_ms,
        consecutive_losses: acc.consecutive_losses,
        consecutive_wins: acc.consecutive_wins,
        pause_until_ms: acc.pause_until_ms,
        daily_pnl: acc.daily_pnl.to_string(),
        pnl_reset_date: acc.pnl_reset_date.clone(),
    };
    drop(acc);
    let tmp = Path::new(log_dir).join("state.json.tmp");
    let dst = Path::new(log_dir).join("state.json");
    if let Ok(json) = serde_json::to_string(&state) {
        let _ = tokio::fs::write(&tmp, &json).await;
        let _ = tokio::fs::rename(&tmp, &dst).await;
    }
}
```

- [ ] **Step 3: Update callers in `Bot::new()`**

```rust
// Load balance from file or use default
let initial_balance = Self::load_balance(&log_dir)
    .await
    .unwrap_or_else(|| decimal("1000.0"));
```

```rust
let saved = Self::load_state(&log_dir).await;
```

- [ ] **Step 4: Update callers in `tick()`**

The `tick()` method has access to `self`, so use `&self.log_dir`:

```rust
// Line ~601: save_state
Self::save_state(&self.log_dir, &self.settler, &self.account).await;

// Line ~604: write_balance
Self::write_balance(&self.log_dir, bal).await;

// Line ~617: trades.csv path
let log_dir = self.log_dir.clone();
let trades_path = Path::new(&log_dir).join("trades.csv");
```

- [ ] **Step 5: Update `start_settlement_checker()`**

Clone `log_dir` into the spawned task (alongside existing clones like `settler`, `account`, etc.):

```rust
fn start_settlement_checker(&self) -> tokio::task::JoinHandle<()> {
    let settler = self.settler.clone();
    let account = self.account.clone();
    let price_source = self.price_source.clone();
    let discovery = self.discovery.clone();
    let btc_tiebreaker_usd = self.config.strategy.btc_tiebreaker_usd;
    let rpc = data::chainlink::rpc_url(self.config.trading.mode);
    let mode = self.config.trading.mode;
    let redeemer = self.redeemer.clone();
    let log_dir = self.log_dir.clone(); // NEW

    tokio::spawn(async move {
        // ... existing code ...

        // Line ~764: save_state call
        Bot::save_state(&log_dir, &settler, &account).await;

        // Line ~779: trades_path
        let trades_path = Path::new(&log_dir).join("trades.csv");

        // Line ~797: write_balance
        Bot::write_balance(&log_dir, bal).await;
    })
}
```

- [ ] **Step 6: Reorder `main()` — config before tracing**

The tracing appender needs the log_dir, which depends on config. Reorder:

```rust
#[tokio::main]
async fn main() -> Result<()> {
    if let Err(e) = rustls::crypto::ring::default_provider().install_default() {
        eprintln!("Failed to install rustls crypto provider: {:?}", e);
        std::process::exit(1);
    }

    load_dotenv();

    // Handle subcommands before full bot init
    if std::env::args().any(|a| a == "--derive-keys") {
        return derive_api_keys().await;
    }
    if std::env::args().any(|a| a == "--redeem-all") {
        return redeem_all().await;
    }
    if let Some(slug) = std::env::args().skip_while(|a| a != "--redeem").nth(1) {
        return redeem_one(&slug).await;
    }

    // Load config FIRST to determine mode (needed for log_dir)
    let config_path = Path::new("config.json");
    let config = if config_path.exists() {
        Config::load(config_path).unwrap_or_else(|e| {
            eprintln!("[INIT] Failed to load config: {}, using defaults", e);
            Config::default()
        })
    } else {
        let cfg = Config::default();
        if let Err(e) = cfg.save(config_path) {
            eprintln!("[INIT] Failed to save default config: {}", e);
        }
        cfg
    };

    config.validate()?;

    // Compute mode-aware log directory
    let log_dir = format!("logs/{}", config.trading.mode);
    tokio::fs::create_dir_all(&log_dir).await.ok();

    let file_appender = tracing_appender::rolling::never(&log_dir, "bot.log");
    let (file_writer, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(file_writer.and(std::io::stderr))
        .with_ansi(false)
        .init();
    tracing::info!("polybot v{}", env!("CARGO_PKG_VERSION"));

    if config.trading.mode.is_live() && config.is_default_non_trading() {
        tracing::warn!(
            "[INIT] Running live mode with default config values; review config.json before trading"
        );
    }

    // Validate credentials for live mode
    if config.trading.mode.is_live() && config.trading.private_key.expose_secret().is_empty() {
        anyhow::bail!("PRIVATE_KEY not set in .env — required for live trading");
    }

    let mut bot = Bot::new(config, log_dir).await?;
    bot.run().await?;

    Ok(())
}
```

Note: `Config::load()` uses `tracing::info!` for TRADING_MODE override, but tracing isn't set up yet at that point. Those messages will be silently dropped — acceptable since the mode is logged again in `Bot::run()`. Use `eprintln!` for pre-tracing errors.

- [ ] **Step 7: Run `cargo check`**

Run: `cargo check 2>&1`
Expected: Clean compile (no errors).

- [ ] **Step 8: Commit**

```bash
git add src/main.rs
git commit -m "feat: route logs to logs/paper/ or logs/live/ based on trading mode"
```

---

### Task 4: Add USDC balance query

**Files:**
- Modify: `src/data/polymarket.rs`

- [ ] **Step 1: Add ERC20 interface and query function**

Add after the existing `ICtfQuery` sol! block (around line 38):

```rust
alloy::sol! {
    #[sol(rpc)]
    interface IERC20 {
        function balanceOf(address account) external view returns (uint256);
    }
}
```

Add the query function (after `CtfRedeemer` impl block):

```rust
/// Query the wallet's USDC balance on Polygon.
/// USDC on Polygon uses 6 decimal places.
pub(crate) async fn query_usdc_balance(rpc_url: &str, wallet: alloy::primitives::Address) -> anyhow::Result<rust_decimal::Decimal> {
    use rust_decimal::Decimal;

    let provider = tokio::time::timeout(
        Duration::from_secs(15),
        alloy::providers::ProviderBuilder::new().connect(rpc_url),
    )
    .await
    .map_err(|_| anyhow::anyhow!("RPC connect timed out querying USDC balance"))?
    .context("RPC connect failed")?;

    let usdc = IERC20::new(POLYGON_USDC, &provider);
    let raw = usdc.balanceOf(wallet).call().await
        .map_err(|e| anyhow::anyhow!("USDC balanceOf failed: {}", e))?;

    // The alloy sol! macro returns the value directly for single-return functions
    // (matching the existing ICtfQuery::balanceOf pattern in check_single()).
    // Convert U256 with 6 decimals to Decimal.
    let raw_u128: u128 = raw.try_into()
        .map_err(|_| anyhow::anyhow!("USDC balance too large for u128"))?;
    Ok(Decimal::from(raw_u128) / Decimal::from(1_000_000u64))
}
```

Note: The existing `ICtfQuery::balanceOf` in `check_single()` (line 265) calls `.is_zero()` directly on the return value, confirming alloy returns U256 directly for single-return functions. If this doesn't compile (alloy version difference), try `raw._0.try_into()` instead.

- [ ] **Step 2: Run `cargo check`**

Run: `cargo check 2>&1`
Expected: Clean compile. If the `_0` accessor doesn't work, adjust based on compiler feedback.

- [ ] **Step 3: Commit**

```bash
git add src/data/polymarket.rs
git commit -m "feat: add on-chain USDC balance query for live mode"
```

---

### Task 5: Balance initialization — paper default + live on-chain

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: No separate constant needed**

`rust_decimal::Decimal` does not support `const` construction. Instead, use the existing `decimal()` helper inline at the call site. No new constant is needed — `decimal("100")` is clear enough.

- [ ] **Step 2: Update `Bot::new()` balance initialization**

Replace the current balance loading (lines 167-171):

```rust
// Paper: load from file or use 100 USDC default
// Live: initial balance will be refreshed from chain before first trade
let initial_balance = if config.trading.mode.is_paper() {
    Self::load_balance(&log_dir)
        .await
        .unwrap_or_else(|| decimal("100"))
} else {
    // For live mode, start with 0; will be refreshed from chain before first tick
    // Try loading saved balance as fallback for display purposes
    Self::load_balance(&log_dir).await.unwrap_or(Decimal::ZERO)
};
tracing::info!("[INIT] Starting balance: ${:.2}", initial_balance);
```

- [ ] **Step 3: Add live balance refresh in `tick()`**

At the top of `tick()` (after `check_daily_reset`), add the live balance refresh:

```rust
async fn tick(&self) -> Result<()> {
    self.account.write().await.check_daily_reset();

    // Live mode: refresh balance from wallet before each decision
    if self.config.trading.mode.is_live() {
        let rpc = data::chainlink::rpc_url(self.config.trading.mode);
        if let Some(ref redeemer) = self.redeemer {
            match redeemer.wallet_address() {
                Ok(wallet) => {
                    match data::polymarket::query_usdc_balance(&rpc, wallet).await {
                        Ok(on_chain_bal) => {
                            self.account.write().await.balance = on_chain_bal;
                        }
                        Err(e) => {
                            tracing::warn!("[BAL] Failed to query on-chain USDC balance: {}", e);
                            // Continue with last known balance
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("[BAL] Failed to derive wallet address: {}", e);
                }
            }
        }
    }

    let mkt = self.market_state.read().await.clone();
    // ... rest of tick() unchanged
```

Note: `self.redeemer` is `Some(...)` in live mode (it's created when `mode.is_live() && PRIVATE_KEY` is set). The `wallet_address()` method already exists on `CtfRedeemer`.

- [ ] **Step 4: Run `cargo check`**

Run: `cargo check 2>&1`
Expected: Clean compile.

- [ ] **Step 5: Run `cargo test`**

Run: `cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat: paper mode defaults to 100 USDC, live mode refreshes balance from wallet on-chain"
```

---

## Chunk 3: watch.sh + Verification

### Task 6: Update watch.sh for mode-aware paths

**Files:**
- Modify: `scripts/watch.sh`

- [ ] **Step 1: Update default paths**

Change the `LOG` and `BALANCE` lines to be mode-aware:

```bash
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MODE="${1:-paper}"
LOG="${ROOT}/logs/${MODE}/bot.log"
SEC="${2:-3}"
```

Update the `BALANCE` line (currently line 23):

```bash
BALANCE=$(cat "${ROOT}/logs/${MODE}/balance" 2>/dev/null || echo "?")
```

Update the usage comment at the top:

```bash
# Usage: scripts/watch.sh [mode] [refresh_seconds]
#   mode: paper (default) or live
```

- [ ] **Step 2: Commit**

```bash
git add scripts/watch.sh
git commit -m "fix: update watch.sh to read from mode-specific log directory"
```

---

### Task 7: Update README

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update repository layout section**

Update the `logs/` section in the repository layout:

```text
logs/                       # Generated at runtime
|- paper/                   # Paper mode data
|  |- bot.log
|  |- trades.csv
|  |- balance
|  `- state.json
`- live/                    # Live mode data
   |- bot.log
   |- trades.csv
   |- balance
   `- state.json
```

- [ ] **Step 2: Update "Logs and Monitoring" section**

Replace references to `logs/bot.log`, `logs/trades.csv`, `logs/balance` with mode-aware paths:

- `logs/<mode>/bot.log`: main runtime log
- `logs/<mode>/trades.csv`: appended on both trade entry and settlement
- `logs/<mode>/balance`: current balance snapshot
- `scripts/watch.sh [mode]`: terminal monitor (defaults to `paper`)

- [ ] **Step 3: Update "Quick Start" section**

Change the monitor command:

```bash
# 4. Monitor logs
scripts/watch.sh          # paper mode (default)
scripts/watch.sh live     # live mode
```

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs: update README for mode-specific log directories"
```

---

### Task 8: Final verification

- [ ] **Step 1: Run full build**

Run: `cargo build --release 2>&1`
Expected: Exit code 0, clean build.

- [ ] **Step 2: Run all tests**

Run: `cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -- -D warnings 2>&1`
Expected: No warnings.

- [ ] **Step 4: Clean up old log files**

```bash
rm -f logs/bot.log logs/trades.csv logs/balance logs/state.json logs/redeem.log
```

Only delete root-level files. The `logs/paper/` and `logs/live/` subdirectories will be created on first run.

- [ ] **Step 5: Commit cleanup**

```bash
git add -A
git commit -m "chore: remove old root-level log files"
```
