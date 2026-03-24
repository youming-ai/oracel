# Decider Optimization Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Optimize the trading decider with minimum edge threshold, multi-timeframe momentum, and dynamic fair value based on BTC history.

**Architecture:** Add two new modules (`btc_history.rs`, `momentum.rs`) to the pipeline, modify `decider.rs` to use them, update config structures, and integrate with `main.rs` for persistence and feedback.

**Tech Stack:** Rust, rust_decimal, serde, tokio

---

## File Structure

| File | Action | Purpose |
|------|--------|---------|
| `src/pipeline/momentum.rs` | Create | Multi-timeframe momentum computation |
| `src/pipeline/btc_history.rs` | Create | BTC window history for dynamic FV |
| `src/pipeline/mod.rs` | Modify | Export new modules |
| `src/pipeline/decider.rs` | Modify | Use new modules, add min_edge |
| `src/pipeline/settler.rs` | Modify | Add window_start_time_ms to PendingPosition |
| `src/config.rs` | Modify | Add new config fields |
| `src/main.rs` | Modify | Wire up BtcHistory, pass to decider, record windows |
| `config.example.json` | Modify | Add new config options |

---

## Chunk 1: New Pipeline Modules

### Task 1: Create momentum.rs with tests

**Files:**
- Create: `src/pipeline/momentum.rs`

- [ ] **Step 1: Write momentum.rs with structs and tests**

```rust
//! Multi-timeframe momentum computation

use crate::pipeline::price_source::PriceTick;
use crate::pipeline::signal::Direction;
use rust_decimal::Decimal;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct MomentumSignal {
    pub short: Decimal,
    pub medium: Decimal,
    pub long: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MomentumMode {
    AllAligned,
}

pub(crate) fn compute_momentum(prices: &[PriceTick], window_secs: u64) -> Decimal {
    if prices.len() < 2 {
        return Decimal::ZERO;
    }

    let now_ms = prices.last().map(|p| p.timestamp_ms).unwrap_or(0);
    let cutoff_ms = now_ms - (window_secs as i64 * 1000);

    let start_price = prices
        .iter()
        .find(|p| p.timestamp_ms >= cutoff_ms)
        .map(|p| p.price)
        .unwrap_or_else(|| prices.first().map(|p| p.price).unwrap_or(Decimal::ZERO));

    let end_price = prices.last().map(|p| p.price).unwrap_or(Decimal::ZERO);

    if start_price == Decimal::ZERO {
        return Decimal::ZERO;
    }

    (end_price - start_price) / start_price
}

pub(crate) fn compute_multi_frame_momentum(
    prices: &[PriceTick],
    short_secs: u64,
    medium_secs: u64,
    long_secs: u64,
) -> MomentumSignal {
    MomentumSignal {
        short: compute_momentum(prices, short_secs),
        medium: compute_momentum(prices, medium_secs),
        long: compute_momentum(prices, long_secs),
    }
}

pub(crate) fn momentum_aligned(
    signal: &MomentumSignal,
    direction: Direction,
    mode: MomentumMode,
) -> bool {
    match mode {
        MomentumMode::AllAligned => {
            let all_up =
                signal.short > Decimal::ZERO && signal.medium > Decimal::ZERO && signal.long > Decimal::ZERO;
            let all_down =
                signal.short < Decimal::ZERO && signal.medium < Decimal::ZERO && signal.long < Decimal::ZERO;

            match direction {
                Direction::Up => all_up,
                Direction::Down => all_down,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_prices() -> Vec<PriceTick> {
        vec![
            PriceTick { price: dec!(100), timestamp_ms: 0 },
            PriceTick { price: dec!(101), timestamp_ms: 30000 },
            PriceTick { price: dec!(102), timestamp_ms: 60000 },
            PriceTick { price: dec!(103), timestamp_ms: 90000 },
            PriceTick { price: dec!(104), timestamp_ms: 120000 },
            PriceTick { price: dec!(105), timestamp_ms: 150000 },
            PriceTick { price: dec!(106), timestamp_ms: 180000 },
        ]
    }

    #[test]
    fn test_compute_momentum_positive() {
        let prices = make_prices();
        let momentum = compute_momentum(&prices, 180);
        assert!(momentum > Decimal::ZERO);
    }

    #[test]
    fn test_compute_momentum_negative() {
        let prices: Vec<PriceTick> = vec![
            PriceTick { price: dec!(100), timestamp_ms: 0 },
            PriceTick { price: dec!(99), timestamp_ms: 60000 },
        ];
        let momentum = compute_momentum(&prices, 60);
        assert!(momentum < Decimal::ZERO);
    }

    #[test]
    fn test_compute_multi_frame_momentum() {
        let prices = make_prices();
        let signal = compute_multi_frame_momentum(&prices, 30, 60, 180);
        
        assert!(signal.short > Decimal::ZERO);
        assert!(signal.medium > Decimal::ZERO);
        assert!(signal.long > Decimal::ZERO);
    }

    #[test]
    fn test_momentum_aligned_all_up() {
        let signal = MomentumSignal {
            short: dec!(0.01),
            medium: dec!(0.02),
            long: dec!(0.03),
        };
        
        assert!(momentum_aligned(&signal, Direction::Up, MomentumMode::AllAligned));
        assert!(!momentum_aligned(&signal, Direction::Down, MomentumMode::AllAligned));
    }

    #[test]
    fn test_momentum_aligned_all_down() {
        let signal = MomentumSignal {
            short: dec!(-0.01),
            medium: dec!(-0.02),
            long: dec!(-0.03),
        };
        
        assert!(momentum_aligned(&signal, Direction::Down, MomentumMode::AllAligned));
        assert!(!momentum_aligned(&signal, Direction::Up, MomentumMode::AllAligned));
    }

    #[test]
    fn test_momentum_not_aligned_mixed() {
        let signal = MomentumSignal {
            short: dec!(0.01),
            medium: dec!(-0.02),
            long: dec!(0.03),
        };
        
        assert!(!momentum_aligned(&signal, Direction::Up, MomentumMode::AllAligned));
        assert!(!momentum_aligned(&signal, Direction::Down, MomentumMode::AllAligned));
    }

    #[test]
    fn test_momentum_insufficient_data() {
        let prices = vec![PriceTick { price: dec!(100), timestamp_ms: 0 }];
        let momentum = compute_momentum(&prices, 60);
        assert_eq!(momentum, Decimal::ZERO);
    }
}
```

- [ ] **Step 2: Run tests to verify**

Run: `cargo test --lib pipeline::momentum`
Expected: All tests pass

- [ ] **Step 3: Commit**

```bash
git add src/pipeline/momentum.rs
git commit -m "feat: add multi-timeframe momentum module"
```

### Task 2: Create btc_history.rs with tests

**Files:**
- Create: `src/pipeline/btc_history.rs`

- [ ] **Step 1: Write btc_history.rs**

```rust
//! BTC window history for dynamic fair value calculation

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BtcWindow {
    pub start_time_ms: i64,
    pub end_time_ms: i64,
    pub start_price: Decimal,
    pub end_price: Decimal,
    pub up_won: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BtcHistory {
    windows: VecDeque<BtcWindow>,
    #[serde(default = "default_max_windows")]
    max_windows: usize,
}

fn default_max_windows() -> usize {
    1000
}

impl Default for BtcHistory {
    fn default() -> Self {
        Self::new(1000)
    }
}

impl BtcHistory {
    pub(crate) fn new(max_windows: usize) -> Self {
        Self {
            windows: VecDeque::with_capacity(max_windows),
            max_windows,
        }
    }

    pub(crate) fn record_window(
        &mut self,
        start_price: Decimal,
        end_price: Decimal,
        start_time_ms: i64,
        end_time_ms: i64,
    ) {
        let up_won = end_price > start_price;
        let window = BtcWindow {
            start_time_ms,
            end_time_ms,
            start_price,
            end_price,
            up_won,
        };

        if self.windows.len() >= self.max_windows {
            self.windows.pop_front();
        }
        self.windows.push_back(window);
    }

    pub(crate) fn dynamic_fair_value(&self, min_samples: usize) -> Option<Decimal> {
        if self.windows.len() < min_samples {
            return None;
        }

        let up_count = self.windows.iter().filter(|w| w.up_won).count();
        let total = self.windows.len();

        Some(Decimal::from(up_count) / Decimal::from(total))
    }

    pub(crate) fn len(&self) -> usize {
        self.windows.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    pub(crate) fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub(crate) fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_record_window() {
        let mut history = BtcHistory::new(100);
        
        history.record_window(dec!(100), dec!(105), 0, 300000);
        
        assert_eq!(history.len(), 1);
        assert!(history.windows[0].up_won);
    }

    #[test]
    fn test_record_window_down() {
        let mut history = BtcHistory::new(100);
        
        history.record_window(dec!(100), dec!(95), 0, 300000);
        
        assert_eq!(history.len(), 1);
        assert!(!history.windows[0].up_won);
    }

    #[test]
    fn test_dynamic_fv_returns_none_when_insufficient_samples() {
        let mut history = BtcHistory::new(100);
        
        for i in 0..10 {
            history.record_window(dec!(100), dec!(101), i * 300000, (i + 1) * 300000);
        }
        
        assert!(history.dynamic_fair_value(20).is_none());
    }

    #[test]
    fn test_dynamic_fv_computes_correct_ratio() {
        let mut history = BtcHistory::new(100);
        
        // 6 up, 4 down -> FV = 0.60
        for i in 0..6 {
            history.record_window(dec!(100), dec!(101), i * 300000, (i + 1) * 300000);
        }
        for i in 6..10 {
            history.record_window(dec!(100), dec!(99), i * 300000, (i + 1) * 300000);
        }
        
        let fv = history.dynamic_fair_value(10).unwrap();
        assert_eq!(fv, dec!(0.6));
    }

    #[test]
    fn test_max_windows_eviction() {
        let mut history = BtcHistory::new(5);
        
        for i in 0..10 {
            history.record_window(dec!(100), dec!(101), i * 300000, (i + 1) * 300000);
        }
        
        assert_eq!(history.len(), 5);
        assert_eq!(history.windows.front().unwrap().start_time_ms, 5 * 300000);
    }

    #[test]
    fn test_to_json_from_json_roundtrip() {
        let mut history = BtcHistory::new(100);
        
        history.record_window(dec!(100), dec!(105), 0, 300000);
        history.record_window(dec!(105), dec!(100), 300000, 600000);
        
        let json = history.to_json().unwrap();
        let restored = BtcHistory::from_json(&json).unwrap();
        
        assert_eq!(restored.len(), 2);
        assert!(restored.windows[0].up_won);
        assert!(!restored.windows[1].up_won);
    }

    #[test]
    fn test_from_json_handles_empty() {
        let json = r#"{"windows":[],"max_windows":1000}"#;
        let history = BtcHistory::from_json(json).unwrap();
        
        assert!(history.is_empty());
        assert_eq!(history.max_windows, 1000);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --lib pipeline::btc_history`
Expected: All tests pass

- [ ] **Step 3: Commit**

```bash
git add src/pipeline/btc_history.rs
git commit -m "feat: add btc_history module for dynamic fair value"
```

### Task 3: Export both modules in mod.rs

**Files:**
- Modify: `src/pipeline/mod.rs`

- [ ] **Step 1: Add module exports**

```rust
pub mod btc_history;
pub mod decider;
pub mod executor;
pub mod momentum;
pub mod price_source;
pub mod settler;
pub mod signal;

#[cfg(test)]
pub mod test_helpers;
```

- [ ] **Step 2: Build to verify**

Run: `cargo build`
Expected: Compilation succeeds

- [ ] **Step 3: Commit**

```bash
git add src/pipeline/mod.rs
git commit -m "feat: export momentum and btc_history modules"
```

---

## Chunk 2: Config Changes

### Task 4: Add config fields for new features

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Add min_edge to StrategyConfig**

Find `StrategyConfig` struct (around line 96) and add after `position_size_usdc`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StrategyConfig {
    #[serde(
        default = "default_extreme_threshold",
        with = "rust_decimal::serde::float"
    )]
    pub extreme_threshold: Decimal,
    #[serde(default = "default_fair_value", with = "rust_decimal::serde::float")]
    pub fair_value: Decimal,
    #[serde(
        default = "default_position_size_usdc",
        with = "rust_decimal::serde::float"
    )]
    pub position_size_usdc: Decimal,
    /// Minimum edge required to trade (default 0.05 = 5%)
    #[serde(default = "default_min_edge", with = "rust_decimal::serde::float")]
    pub min_edge: Decimal,
    /// Enable BTC momentum filter
    #[serde(default)]
    pub momentum_filter: MomentumFilterConfig,
    /// Enable dynamic fair value based on volatility
    #[serde(default)]
    pub dynamic_fair_value: DynamicFairValueConfig,
    /// Enable dynamic fair value based on BTC history
    #[serde(default)]
    pub btc_history: BtcHistoryConfig,
}

fn default_min_edge() -> Decimal {
    dec("0.05")
}
```

- [ ] **Step 2: Add multi-timeframe momentum config**

Replace `MomentumFilterConfig` struct (around line 118) with:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct MomentumFilterConfig {
    /// Enable momentum filtering
    #[serde(default)]
    pub enabled: bool,
    /// Short timeframe for momentum (seconds)
    #[serde(default = "default_momentum_short_secs")]
    pub short_secs: u64,
    /// Medium timeframe for momentum (seconds)
    #[serde(default = "default_momentum_medium_secs")]
    pub medium_secs: u64,
    /// Long timeframe for momentum (seconds)
    #[serde(default = "default_momentum_long_secs")]
    pub long_secs: u64,
    /// Minimum momentum alignment (0.0 = disabled, higher = stricter)
    #[serde(
        default = "default_momentum_threshold",
        with = "rust_decimal::serde::float"
    )]
    pub threshold: Decimal,
    /// Legacy field for backward compatibility
    #[serde(default = "default_momentum_window_secs")]
    pub window_secs: u64,
}

fn default_momentum_short_secs() -> u64 {
    30
}
fn default_momentum_medium_secs() -> u64 {
    60
}
fn default_momentum_long_secs() -> u64 {
    180
}
fn default_momentum_window_secs() -> u64 {
    60
}
fn default_momentum_threshold() -> Decimal {
    dec("0.002")
}
```

- [ ] **Step 3: Add BtcHistoryConfig struct**

Add after `DynamicFairValueConfig`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct BtcHistoryConfig {
    /// Enable dynamic fair value based on BTC history
    #[serde(default)]
    pub enabled: bool,
    /// Minimum samples required before using dynamic FV
    #[serde(default = "default_btc_history_min_samples")]
    pub min_samples: usize,
    /// Maximum number of windows to keep
    #[serde(default = "default_btc_history_max_windows")]
    pub max_windows: usize,
}

fn default_btc_history_min_samples() -> usize {
    20
}
fn default_btc_history_max_windows() -> usize {
    1000
}
```

- [ ] **Step 4: Update StrategyConfig Default impl**

Find `impl Default for StrategyConfig` (around line 324) and update:

```rust
impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            extreme_threshold: dec("0.80"),
            fair_value: dec("0.50"),
            position_size_usdc: dec("1.0"),
            min_edge: dec("0.05"),
            momentum_filter: MomentumFilterConfig::default(),
            dynamic_fair_value: DynamicFairValueConfig::default(),
            btc_history: BtcHistoryConfig::default(),
        }
    }
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib config`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add src/config.rs
git commit -m "feat: add min_edge and btc_history config options"
```

---

## Chunk 3: Decider Modifications

### Task 5: Update DeciderConfig and decide function

**Files:**
- Modify: `src/pipeline/decider.rs`

- [ ] **Step 1: Add imports at top of file**

```rust
use crate::pipeline::btc_history::BtcHistory;
use crate::pipeline::momentum::{compute_multi_frame_momentum, momentum_aligned, MomentumMode};
```

- [ ] **Step 2: Update DeciderConfig struct**

Replace `DeciderConfig` struct (around line 33) with:

```rust
#[derive(Debug, Clone)]
pub(crate) struct DeciderConfig {
    /// Fixed position size per trade (USDC)
    pub position_size_usdc: Decimal,
    /// Market price threshold to consider "extreme" (e.g. 0.80)
    pub extreme_threshold: Decimal,
    /// Fair value assumption for binary outcome (e.g. 0.50)
    pub fair_value: Decimal,
    /// Minimum edge required to trade (default 0.05)
    pub min_edge: Decimal,
    /// Momentum filter settings
    pub momentum_filter_enabled: bool,
    pub momentum_short_secs: u64,
    pub momentum_medium_secs: u64,
    pub momentum_long_secs: u64,
    /// Dynamic fair value settings (volatility-based)
    pub dynamic_fv_enabled: bool,
    pub volatility_window_secs: u64,
    pub volatility_weight: Decimal,
    /// BTC history dynamic FV settings
    pub btc_history_enabled: bool,
    pub btc_history_min_samples: usize,
    /// Risk controls
    pub daily_loss_limit_usdc: Decimal,
}
```

- [ ] **Step 3: Update Default impl for DeciderConfig**

Replace `Default` impl (around line 53):

```rust
impl Default for DeciderConfig {
    fn default() -> Self {
        Self {
            position_size_usdc: decimal("1.0"),
            extreme_threshold: decimal("0.80"),
            fair_value: decimal("0.50"),
            min_edge: decimal("0.05"),
            momentum_filter_enabled: false,
            momentum_short_secs: 30,
            momentum_medium_secs: 60,
            momentum_long_secs: 180,
            dynamic_fv_enabled: false,
            volatility_window_secs: 300,
            volatility_weight: decimal("0.1"),
            btc_history_enabled: false,
            btc_history_min_samples: 20,
            daily_loss_limit_usdc: decimal("0"),
        }
    }
}
```

- [ ] **Step 4: Update decide function signature and body**

Replace `decide` function (around line 227) with:

```rust
pub(crate) fn decide(
    ctx: &DecideContext,
    account: &AccountState,
    cfg: &DeciderConfig,
    btc_history: &BtcHistory,
) -> Decision {
    // 1. Balance check
    if account.balance <= Decimal::ZERO {
        return Decision::Pass("insufficient_balance".into());
    }

    // 2. Daily loss limit check
    if cfg.daily_loss_limit_usdc > Decimal::ZERO && account.daily_pnl < -cfg.daily_loss_limit_usdc {
        return Decision::Pass(format!(
            "daily_loss_limit_{:.0}",
            account.daily_pnl.round_dp(0)
        ));
    }

    // 3. Need market data
    let (yes, no) = match (ctx.market_yes, ctx.market_no) {
        (Some(y), Some(n)) if y > decimal("0.01") && n > decimal("0.01") => (y, n),
        _ => return Decision::Pass("no_market_data".into()),
    };

    let total = yes + no;
    if total <= Decimal::ZERO {
        return Decision::Pass("no_liquidity".into());
    }

    // Spread check: if yes + no < 0.80, liquidity is too thin
    if total < decimal("0.80") {
        return Decision::Pass(format!(
            "wide_spread_{:.0}%",
            ((Decimal::ONE - total) * decimal("100")).round_dp(0)
        ));
    }

    let mkt_up = yes / total;

    // 4. Compute BTC volatility for dynamic FV
    let btc_volatility = compute_volatility(&ctx.btc_prices, cfg.volatility_window_secs);

    // 5. Market extreme check - time-weighted threshold
    let late_floor = decimal("0.90");
    let late_threshold = if cfg.extreme_threshold > late_floor {
        cfg.extreme_threshold
    } else {
        late_floor
    };
    let extreme_thr = if ctx.remaining_ms > 180_000 {
        cfg.extreme_threshold
    } else if ctx.remaining_ms > 120_000 {
        let frac = Decimal::from(180_000 - ctx.remaining_ms) / Decimal::from(60_000_i64);
        cfg.extreme_threshold + (late_threshold - cfg.extreme_threshold) * frac
    } else {
        late_threshold
    };

    // 6. Determine direction based on market extreme
    let (base_direction, cheap_price) = if mkt_up > extreme_thr {
        (Direction::Down, no / total)
    } else if mkt_up < (Decimal::ONE - extreme_thr) {
        (Direction::Up, yes / total)
    } else {
        return Decision::Pass(format!(
            "not_extreme_{}%",
            (mkt_up * decimal("100")).round_dp(0)
        ));
    };

    // 7. Multi-timeframe momentum filter
    let momentum_signal = compute_multi_frame_momentum(
        &ctx.btc_prices,
        cfg.momentum_short_secs,
        cfg.momentum_medium_secs,
        cfg.momentum_long_secs,
    );

    if cfg.momentum_filter_enabled {
        if !momentum_aligned(&momentum_signal, base_direction, MomentumMode::AllAligned) {
            return Decision::Pass(format!(
                "momentum_not_aligned_{:+.1}%_{:+.1}%_{:+.1}%",
                (momentum_signal.short * decimal("100")).round_dp(1),
                (momentum_signal.medium * decimal("100")).round_dp(1),
                (momentum_signal.long * decimal("100")).round_dp(1)
            ));
        }
    }

    // 8. Dynamic fair value: BTC history takes priority over volatility-based
    let effective_fair_value = if cfg.btc_history_enabled {
        btc_history
            .dynamic_fair_value(cfg.btc_history_min_samples)
            .unwrap_or_else(|| {
                if cfg.dynamic_fv_enabled && btc_volatility > Decimal::ZERO {
                    let boost = btc_volatility * cfg.volatility_weight;
                    cfg.fair_value + boost
                } else {
                    cfg.fair_value
                }
            })
    } else if cfg.dynamic_fv_enabled && btc_volatility > Decimal::ZERO {
        let boost = btc_volatility * cfg.volatility_weight;
        cfg.fair_value + boost
    } else {
        cfg.fair_value
    };

    // 9. Calculate edge
    let edge = effective_fair_value - cheap_price;

    // 10. Minimum edge check
    if edge < cfg.min_edge {
        return Decision::Pass(format!("edge_too_low_{:.1}%", (edge * decimal("100")).round_dp(1)));
    }

    // 11. Calculate payoff ratio
    let payoff_ratio = if cheap_price > Decimal::ZERO {
        (Decimal::ONE - cheap_price) / cheap_price
    } else {
        Decimal::new(99, 0)
    };

    Decision::Trade {
        direction: base_direction,
        size_usdc: cfg.position_size_usdc,
        edge,
        payoff_ratio,
        btc_momentum: momentum_signal.medium,
        btc_volatility,
    }
}
```

- [ ] **Step 5: Remove compute_momentum function**

Delete the `compute_momentum` function (around line 134-157) as it's now in momentum.rs.

- [ ] **Step 6: Update tests to use new signature**

Add helper at the start of tests module:

```rust
fn default_btc_history() -> BtcHistory {
    BtcHistory::new(100)
}
```

Then update all `decide()` calls in tests to include `&default_btc_history()` as the 4th argument. For example:

```rust
let decision = decide(&ctx, &account, &cfg, &default_btc_history());
```

- [ ] **Step 7: Add new tests**

Add these new tests at the end of the tests module:

```rust
#[test]
fn test_min_edge_rejects_low_edge() {
    let account = AccountState::new(d("1000"));
    let mut cfg = DeciderConfig {
        min_edge: d("0.10"), // Require 10% edge
        ..DeciderConfig::default()
    };
    cfg.extreme_threshold = d("0.64"); // Lower threshold to get trade
    
    let mut ctx = default_ctx();
    ctx.market_yes = Some(d("0.60"));
    ctx.market_no = Some(d("0.40"));
    
    let decision = decide(&ctx, &account, &cfg, &default_btc_history());
    
    match decision {
        Decision::Pass(reason) => assert!(reason.starts_with("edge_too_low")),
        Decision::Trade { .. } => panic!("expected pass due to low edge"),
    }
}

#[test]
fn test_min_edge_allows_high_edge() {
    let account = AccountState::new(d("1000"));
    let mut cfg = DeciderConfig {
        min_edge: d("0.05"),
        ..DeciderConfig::default()
    };
    cfg.extreme_threshold = d("0.64");
    
    let mut ctx = default_ctx();
    ctx.market_yes = Some(d("0.70"));
    ctx.market_no = Some(d("0.30"));
    
    let decision = decide(&ctx, &account, &cfg, &default_btc_history());
    
    match decision {
        Decision::Trade { .. } => {}
        Decision::Pass(reason) => panic!("expected trade but got pass: {}", reason),
    }
}

#[test]
fn test_multi_frame_momentum_filter() {
    let account = AccountState::new(d("1000"));
    let cfg = DeciderConfig {
        momentum_filter_enabled: true,
        momentum_short_secs: 30,
        momentum_medium_secs: 60,
        momentum_long_secs: 180,
        ..DeciderConfig::default()
    };
    
    // All momentum up - should reject DOWN trade
    let mut ctx = default_ctx();
    ctx.btc_prices = vec![
        PriceTick { price: d("70000"), timestamp_ms: 0 },
        PriceTick { price: d("70200"), timestamp_ms: 30000 },
        PriceTick { price: d("70400"), timestamp_ms: 60000 },
        PriceTick { price: d("70600"), timestamp_ms: 90000 },
        PriceTick { price: d("70800"), timestamp_ms: 120000 },
        PriceTick { price: d("71000"), timestamp_ms: 150000 },
        PriceTick { price: d("71200"), timestamp_ms: 180000 },
    ];
    
    let decision = decide(&ctx, &account, &cfg, &default_btc_history());
    
    match decision {
        Decision::Pass(reason) => assert!(reason.starts_with("momentum_not_aligned")),
        Decision::Trade { .. } => panic!("expected pass due to momentum filter"),
    }
}

#[test]
fn test_dynamic_fv_uses_history_when_available() {
    let account = AccountState::new(d("1000"));
    let cfg = DeciderConfig {
        btc_history_enabled: true,
        btc_history_min_samples: 3,
        ..DeciderConfig::default()
    };
    
    let mut history = BtcHistory::new(100);
    // 2 up, 1 down -> FV = 0.666...
    history.record_window(d("100"), d("101"), 0, 300000);
    history.record_window(d("101"), d("102"), 300000, 600000);
    history.record_window(d("102"), d("101"), 600000, 900000);
    
    let ctx = default_ctx();
    let decision = decide(&ctx, &account, &cfg, &history);
    
    // With FV ~0.67 and cheap_price 0.15, edge should be ~0.52
    match decision {
        Decision::Trade { edge, .. } => {
            assert!(edge > d("0.5"));
        }
        Decision::Pass(reason) => panic!("expected trade but got pass: {}", reason),
    }
}
```

- [ ] **Step 8: Run tests**

Run: `cargo test --lib pipeline::decider`
Expected: All tests pass

- [ ] **Step 9: Commit**

```bash
git add src/pipeline/decider.rs
git commit -m "feat: update decider with min_edge, multi-frame momentum, and btc_history"
```

---

## Chunk 4: PendingPosition Modification

### Task 6: Add window_start_time_ms to PendingPosition

**Files:**
- Modify: `src/pipeline/settler.rs`

- [ ] **Step 1: Add field to PendingPosition struct**

Find `PendingPosition` struct (around line 10) and add:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct PendingPosition {
    pub direction: Direction,
    pub size_usdc: Decimal,
    pub entry_price: Decimal,
    pub filled_shares: Decimal,
    pub cost: Decimal,
    pub settlement_time_ms: i64,
    pub entry_btc_price: Decimal,
    pub condition_id: Arc<str>,
    pub market_slug: Arc<str>,
    /// BTC price at window start time (for history tracking)
    #[serde(default)]
    pub window_start_btc_price: Decimal,
}
```

- [ ] **Step 2: Update tests to include new field**

Update `sample_pending()` helper in tests to include the new field:

```rust
fn sample_pending() -> PendingPosition {
    PendingPosition {
        direction: Direction::Up,
        size_usdc: d("5.0"),
        entry_price: d("0.20"),
        filled_shares: d("25.00"),
        cost: d("5.0"),
        settlement_time_ms: 0,
        entry_btc_price: d("70000"),
        condition_id: "cid".into(),
        market_slug: "btc-updown-5m-1".into(),
        window_start_btc_price: d("69800"),
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib pipeline::settler`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add src/pipeline/settler.rs
git commit -m "feat: add window_start_btc_price to PendingPosition"
```

---

## Chunk 5: main.rs Integration

### Task 7: Add BtcHistory to Bot and wire up decider

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add import**

Add at the top with other pipeline imports (around line 50):

```rust
use pipeline::btc_history::BtcHistory;
```

- [ ] **Step 2: Add btc_history to PersistState**

Find `PersistState` struct (around line 32) and add:

```rust
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct PersistState {
    pending_positions: Vec<PendingPosition>,
    #[serde(default)]
    consecutive_losses: u32,
    #[serde(default)]
    consecutive_wins: u32,
    #[serde(default)]
    total_wins: u32,
    #[serde(default)]
    total_losses: u32,
    #[serde(default)]
    btc_history_json: Option<String>,
}
```

- [ ] **Step 3: Add btc_history to Bot struct**

Find `Bot` struct (around line 100) and add:

```rust
struct Bot {
    config: Config,
    log_dir: String,
    price_source: Arc<PriceSource>,
    polymarket: Arc<PolymarketClient>,
    discovery: Arc<MarketDiscovery>,
    state: Arc<RwLock<BotState>>,
    account: Arc<RwLock<AccountState>>,
    settler: Arc<RwLock<Settler>>,
    executor: Executor,
    redeemer: Option<Arc<CtfRedeemer>>,
    balance_checker: Option<BalanceChecker>,
    market_state: Arc<RwLock<MarketState>>,
    btc_history: Arc<RwLock<BtcHistory>>,
}
```

- [ ] **Step 4: Initialize btc_history in Bot::new**

Find `Bot::new` function and add after settler initialization (around line 230):

```rust
let btc_history = Arc::new(RwLock::new(BtcHistory::new(config.strategy.btc_history.max_windows)));
```

And ensure it's included in the return statement at the end of `Bot::new`.

- [ ] **Step 5: Update decider_cfg method**

Find `decider_cfg` method (around line 417) and update:

```rust
fn decider_cfg(&self) -> DeciderConfig {
    DeciderConfig {
        position_size_usdc: self.config.strategy.position_size_usdc,
        extreme_threshold: self.config.strategy.extreme_threshold,
        fair_value: self.config.strategy.fair_value,
        min_edge: self.config.strategy.min_edge,
        momentum_filter_enabled: self.config.strategy.momentum_filter.enabled,
        momentum_short_secs: self.config.strategy.momentum_filter.short_secs,
        momentum_medium_secs: self.config.strategy.momentum_filter.medium_secs,
        momentum_long_secs: self.config.strategy.momentum_filter.long_secs,
        dynamic_fv_enabled: self.config.strategy.dynamic_fair_value.enabled,
        volatility_window_secs: self.config.strategy.dynamic_fair_value.volatility_window_secs,
        volatility_weight: self.config.strategy.dynamic_fair_value.volatility_weight,
        btc_history_enabled: self.config.strategy.btc_history.enabled,
        btc_history_min_samples: self.config.strategy.btc_history.min_samples,
        daily_loss_limit_usdc: self.config.risk.daily_loss_limit_usdc,
    }
}
```

- [ ] **Step 6: Update decide() call in tick()**

Find the `decide()` call (around line 576) and update:

```rust
let btc_history_read = self.btc_history.read().await.clone();
let decision = decider::decide(&decide_ctx, &account_read, &decider_cfg, &btc_history_read);
```

- [ ] **Step 7: Update PendingPosition creation in tick()**

Find where `PendingPosition` is created (around line 682) and add the new field:

```rust
self.settler.write().await.add_position(PendingPosition {
    direction: order.direction,
    size_usdc: order.size_usdc,
    entry_price: order.entry_price,
    filled_shares: order.filled_shares,
    cost: order.cost,
    settlement_time_ms: order.settlement_time_ms,
    entry_btc_price: order.entry_btc_price,
    condition_id: Arc::clone(&mkt.condition_id),
    market_slug: Arc::clone(&mkt.market_slug),
    window_start_btc_price: btc_price, // Current BTC price at window start
});
```

- [ ] **Step 8: Update save_state function signature and body**

Find `save_state` function and update to include btc_history:

```rust
async fn save_state(
    log_dir: &str, 
    settler: &Arc<RwLock<Settler>>, 
    account: &Arc<RwLock<AccountState>>, 
    btc_history: &Arc<RwLock<BtcHistory>>
) {
    let btc_history_json = btc_history.read().await.to_json().ok();
    let state = PersistState {
        pending_positions: settler.read().await.pending_positions(),
        consecutive_losses: account.read().await.consecutive_losses,
        consecutive_wins: account.read().await.consecutive_wins,
        total_wins: account.read().await.total_wins,
        total_losses: account.read().await.total_losses,
        btc_history_json,
    };
    let json = serde_json::to_string_pretty(&state).unwrap_or_default();
    let path = Path::new(log_dir).join("state.json");
    let temp_path = Path::new(log_dir).join("state.json.tmp");
    match tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        std::fs::write(&temp_path, json)?;
        std::fs::rename(&temp_path, path)?;
        Ok(())
    })
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!("[STATE] save failed: {}", e),
        Err(e) => tracing::warn!("[STATE] save task failed: {}", e),
    }
}
```

- [ ] **Step 9: Update all save_state call sites**

Find all calls to `Self::save_state` and add `&self.btc_history` as the 4th argument:
1. In `tick()` after trade execution (around line 694)
2. In `start_settlement_checker()` after settlement (around line 840)

- [ ] **Step 10: Update load_state to restore btc_history**

Find where state is loaded (in `Bot::new` or a restore function) and add btc_history restoration:

```rust
// After loading PersistState from file
if let Some(json) = &saved.btc_history_json {
    if let Ok(history) = BtcHistory::from_json(json) {
        *self.btc_history.write().await = history;
    }
}
```

- [ ] **Step 11: Build and test**

Run: `cargo build && cargo test`
Expected: Compilation succeeds, tests pass

- [ ] **Step 12: Commit**

```bash
git add src/main.rs
git commit -m "feat: integrate btc_history with bot state and decider"
```

---

## Chunk 6: Settlement Integration

### Task 8: Record BTC window results on settlement

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add btc_history to start_settlement_checker captures**

Find `start_settlement_checker` function (around line 760) and add to the captures:

```rust
fn start_settlement_checker(&self) -> tokio::task::JoinHandle<()> {
    let settler = self.settler.clone();
    let account = self.account.clone();
    let price_source = self.price_source.clone();
    let discovery = self.discovery.clone();
    let redeemer = self.redeemer.clone();
    let log_dir = self.log_dir.clone();
    let btc_history = self.btc_history.clone();  // Add this
    // ...
}
```

- [ ] **Step 2: Add window recording after settlement**

Find the section where results are processed (after the `for r in &results` loop that calls `acc.record_settlement(r)`). Add this code block after that loop but before `Self::save_state`:

```rust
// Record BTC window results for dynamic FV
{
    let mut history = btc_history.write().await;
    for pos in &due {
        // Get current BTC price as window end price
        if let Some(current_btc) = price_source.latest().await {
            // Calculate window start time from settlement_time_ms (5 min = 300000 ms before)
            let window_start_ms = pos.settlement_time_ms - 300000;
            let window_end_ms = pos.settlement_time_ms;
            
            // Use the stored window_start_btc_price (from Task 6)
            // If window_start_btc_price is zero, use entry_btc_price as fallback
            let start_price = if pos.window_start_btc_price > Decimal::ZERO {
                pos.window_start_btc_price
            } else {
                pos.entry_btc_price
            };
            
            history.record_window(
                start_price,
                current_btc,
                window_start_ms,
                window_end_ms,
            );
        }
    }
}
```

Note: This uses `window_start_btc_price` from `PendingPosition` (added in Task 6) for accurate tracking. The `entry_btc_price` is used as fallback for backward compatibility.

- [ ] **Step 3: Verify save_state call includes btc_history**

The save_state call should already include `&btc_history` from Task 7, Step 9. Find and verify:

```rust
Self::save_state(&log_dir, &settler, &account, &btc_history).await;
```

- [ ] **Step 4: Build and test**

Run: `cargo build && cargo test`
Expected: Compilation succeeds

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: record BTC window results on settlement"
```

---

## Chunk 7: Config Example Update

### Task 9: Update config.example.json

**Files:**
- Modify: `config.example.json`

- [ ] **Step 1: Add new config fields**

Update the strategy section:

```json
{
  "strategy": {
    "extreme_threshold": 0.88,
    "fair_value": 0.5,
    "position_size_usdc": 1.0,
    "min_edge": 0.05,
    "momentum_filter": {
      "enabled": true,
      "short_secs": 30,
      "medium_secs": 60,
      "long_secs": 180,
      "threshold": 0.003
    },
    "dynamic_fair_value": {
      "enabled": true,
      "volatility_window_secs": 300,
      "volatility_weight": 0.15
    },
    "btc_history": {
      "enabled": true,
      "min_samples": 20,
      "max_windows": 1000
    }
  }
}
```

- [ ] **Step 2: Commit**

```bash
git add config.example.json
git commit -m "docs: update config.example.json with new options"
```

---

## Chunk 8: Final Verification

### Task 10: Full test and lint

- [ ] **Step 1: Run full CI check**

Run: `cargo build --locked && cargo test --locked && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo fmt --all -- --check`
Expected: All checks pass

- [ ] **Step 2: Fix any issues**

If any issues are found, fix them and re-run the checks.

- [ ] **Step 3: Final commit (if needed)**

```bash
git add -A
git commit -m "fix: resolve lint and test issues"
```

---

## Summary

This plan implements:

1. **momentum.rs** - Multi-timeframe momentum with 30s/60s/180s windows
2. **btc_history.rs** - BTC window history for dynamic fair value calculation
3. **Config changes** - min_edge, multi-frame momentum settings, btc_history settings
4. **Decider changes** - Uses new modules, adds min_edge check
5. **PendingPosition change** - Adds window_start_btc_price for accurate tracking
6. **Main.rs changes** - Integrates BtcHistory with bot state and settlement
7. **config.example.json** - Updated with new options

Total estimated implementation time: 2-3 hours
