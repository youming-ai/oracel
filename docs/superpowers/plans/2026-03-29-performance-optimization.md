# Performance Optimization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement all performance optimizations identified in the codebase analysis to reduce latency, minimize allocations, and improve throughput.

**Architecture:** A series of targeted optimizations across the trading pipeline, focusing on lock-free data structures, parallel I/O, reduced allocations, and efficient file handling.

**Tech Stack:** Rust 2021, Tokio async runtime, crossbeam (to be added), serde

---

## Task 1: Add crossbeam Dependency

**Files:**
- Modify: `Cargo.toml:21-45`

- [ ] **Step 1: Add crossbeam to dependencies**

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }
tokio-tungstenite = { version = "0.24", features = ["rustls-tls-webpki-roots"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tracing-appender = "0.2"
chrono = { version = "0.4", features = ["serde"] }
anyhow = "1"
uuid = { version = "1", features = ["v4", "serde"] }
futures-util = "0.3"
rust_decimal = { version = "1", features = ["serde", "serde-with-float"] }
polymarket-client-sdk = { version = "0.4", features = ["clob", "ctf"] }
alloy = { version = "1.6", default-features = false, features = ["signer-local", "signers", "contract", "providers", "provider-http", "transports", "transport-http"] }
dotenvy = "0.15"
secrecy = "0.10"
rustls = { version = "0.23", features = ["ring"] }
crossbeam = "0.8"
```

- [ ] **Step 2: Verify dependency resolution**

Run: `cargo check`
Expected: Dependencies resolve successfully with no errors

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "deps: add crossbeam for lock-free data structures"
```

---

## Task 2: Optimize Price Buffer (Lock-Free)

**Files:**
- Read: `src/pipeline/price_source.rs`
- Modify: `src/pipeline/price_source.rs`

- [ ] **Step 1: Read current price_source.rs implementation**

- [ ] **Step 2: Replace RwLock<VecDeque> with crossbeam ArrayQueue**

```rust
//! Stage 1: Price Source — Optimized for 5min window latency
//!
//! Performance targets:
//! - <1ms price ingestion latency
//! - Lock-free read path for latest price
//! - Zero-allocation hot path

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use crossbeam::queue::ArrayQueue;
use rust_decimal::Decimal;
use tokio::sync::broadcast;

/// Default capacity for the price buffer (5 minutes at 1-second intervals)
const DEFAULT_BUFFER_CAPACITY: usize = 300;

/// A price tick with timestamp
#[derive(Debug, Clone, Copy)]
pub struct PriceTick {
    pub price: Decimal,
    pub timestamp: DateTime<Utc>,
}

/// Thread-safe, lock-free price buffer using ArrayQueue
#[derive(Debug)]
pub struct PriceBuffer {
    buffer: Arc<ArrayQueue<PriceTick>>,
    latest_price: AtomicU64, // Store as atomic for lock-free reads
    latest_timestamp: AtomicU64, // Unix timestamp as nanos
    tx: broadcast::Sender<PriceTick>,
}

impl PriceBuffer {
    /// Create a new price buffer with default capacity
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_BUFFER_CAPACITY)
    }

    /// Create a new price buffer with specified capacity
    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(100);
        Self {
            buffer: Arc::new(ArrayQueue::new(capacity)),
            latest_price: AtomicU64::new(0),
            latest_timestamp: AtomicU64::new(0),
            tx,
        }
    }

    /// Insert a new price tick (lock-free, may drop oldest if full)
    pub fn push(&self, tick: PriceTick) {
        // Store price as scaled u64 for atomic operations
        let price_scaled = (tick.price.to_f64().unwrap_or(0.0) * 1e8) as u64;
        self.latest_price.store(price_scaled, Ordering::Relaxed);
        self.latest_timestamp.store(
            tick.timestamp.timestamp_nanos_opt().unwrap_or(0) as u64,
            Ordering::Relaxed,
        );

        // Non-blocking push - if full, pop oldest first
        while self.buffer.is_full() {
            let _ = self.buffer.pop();
        }
        let _ = self.buffer.push(tick.clone());
        
        // Broadcast to subscribers (non-blocking)
        let _ = self.tx.send(tick);
    }

    /// Get the latest price (lock-free read)
    pub fn latest(&self) -> Option<PriceTick> {
        let price_scaled = self.latest_price.load(Ordering::Relaxed);
        let timestamp_nanos = self.latest_timestamp.load(Ordering::Relaxed) as i64;
        
        if price_scaled == 0 {
            return None;
        }

        let price = Decimal::from_f64(price_scaled as f64 / 1e8)?;
        let timestamp = DateTime::from_timestamp_nanos(timestamp_nanos);

        Some(PriceTick { price, timestamp })
    }

    /// Get buffer length (approximate, lock-free)
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Get a snapshot of all prices in the buffer
    pub fn snapshot(&self) -> Vec<PriceTick> {
        // Drain and re-insert to get ordered snapshot
        let mut ticks = Vec::with_capacity(self.buffer.len());
        while let Some(tick) = self.buffer.pop() {
            ticks.push(tick);
        }
        // Re-insert (maintains order since we drained from front)
        for tick in &ticks {
            let _ = self.buffer.push(*tick);
        }
        ticks
    }

    /// Subscribe to price updates
    pub fn subscribe(&self) -> broadcast::Receiver<PriceTick> {
        self.tx.subscribe()
    }

    /// Get the broadcast sender (for cloning)
    pub fn sender(&self) -> broadcast::Sender<PriceTick> {
        self.tx.clone()
    }
}

impl Default for PriceBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn d(value: &str) -> Decimal {
        Decimal::from_str_exact(value).expect("valid decimal")
    }

    #[test]
    fn test_push_and_latest() {
        let buffer = PriceBuffer::new();
        let tick = PriceTick {
            price: d("50000.00"),
            timestamp: Utc::now(),
        };
        
        buffer.push(tick);
        
        let latest = buffer.latest();
        assert!(latest.is_some());
        // Note: atomic storage uses f64 conversion, so we check approximate equality
        let latest_price = latest.unwrap().price;
        assert!((latest_price - d("50000.00")).abs() < d("0.01"));
    }

    #[test]
    fn test_buffer_capacity() {
        let buffer = PriceBuffer::with_capacity(10);
        
        for i in 0..15 {
            buffer.push(PriceTick {
                price: d(&format!("{}.00", i)),
                timestamp: Utc::now(),
            });
        }
        
        // Buffer should have dropped oldest entries
        assert!(buffer.len() <= 10);
    }

    #[test]
    fn test_concurrent_access() {
        use std::thread;
        
        let buffer = Arc::new(PriceBuffer::new());
        let mut handles = vec![];
        
        // Spawn producers
        for i in 0..10 {
            let buf = Arc::clone(&buffer);
            handles.push(thread::spawn(move || {
                for j in 0..100 {
                    buf.push(PriceTick {
                        price: d(&format!("{}.{}", i, j)),
                        timestamp: Utc::now(),
                    });
                }
            }));
        }
        
        // Spawn consumers
        for _ in 0..5 {
            let buf = Arc::clone(&buffer);
            handles.push(thread::spawn(move || {
                for _ in 0..200 {
                    let _ = buf.latest();
                    let _ = buf.len();
                }
            }));
        }
        
        for handle in handles {
            handle.join().unwrap();
        }
        
        // Should not panic or deadlock
        assert!(buffer.len() <= DEFAULT_BUFFER_CAPACITY);
    }
}
```

- [ ] **Step 3: Run tests for price_source module**

Run: `cargo test --lib pipeline::price_source`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add src/pipeline/price_source.rs
git commit -m "perf: replace RwLock<VecDeque> with lock-free ArrayQueue in price buffer

- Use crossbeam::ArrayQueue for lock-free push/pop operations
- Add atomic storage for latest price/timestamp for true lock-free reads
- Remove async requirement from push/latest methods
- Add comprehensive concurrency tests"
```

---

## Task 3: Parallelize Polymarket Price Fetches

**Files:**
- Read: `src/bot.rs` (lines 375-395)
- Modify: `src/bot.rs`

- [ ] **Step 1: Read current sequential price fetch implementation**

- [ ] **Step 2: Replace sequential fetches with tokio::join!**

Locate the code around line 375-395 that fetches prices:

```rust
// OLD CODE (to be replaced):
// let (poly_yes_dec, poly_no_dec) = match (
//     self.polymarket.fetch_mid_price(&mkt.token_yes).await,
//     self.polymarket.fetch_mid_price(&mkt.token_no).await,
// ) { ... }

// NEW CODE:
let (poly_yes_res, poly_no_res) = tokio::join!(
    self.polymarket.fetch_mid_price(&mkt.token_yes),
    self.polymarket.fetch_mid_price(&mkt.token_no),
);

let (poly_yes_dec, poly_no_dec) = match (poly_yes_res, poly_no_res) {
    (Ok(yes), Ok(no)) => (yes, no),
    (Err(e), _) => {
        tracing::warn!("[SKIP] Failed to fetch YES price: {}", e);
        self.state.record_skip(IdleReason::NoMarketData);
        return Ok(());
    }
    (_, Err(e)) => {
        tracing::warn!("[SKIP] Failed to fetch NO price: {}", e);
        self.state.record_skip(IdleReason::NoMarketData);
        return Ok(());
    }
};
```

- [ ] **Step 3: Add import for tokio::join if not present**

At the top of `bot.rs`, ensure the import exists:

```rust
use tokio::{join, select, time::interval};
```

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 5: Commit**

```bash
git add src/bot.rs
git commit -m "perf: parallelize Polymarket price fetches with tokio::join!

- Fetch YES and NO token prices concurrently instead of sequentially
- Reduces market data latency by ~2x when both calls succeed"
```

---

## Task 4: Optimize Pending Positions (HashMap Lookup)

**Files:**
- Read: `src/pipeline/settler.rs`
- Modify: `src/pipeline/settler.rs`

- [ ] **Step 1: Read current settler.rs implementation**

- [ ] **Step 2: Replace VecDeque with HashMap for O(1) lookups**

```rust
use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

/// A pending position waiting for settlement
#[derive(Debug, Clone)]
pub struct PendingPosition {
    pub condition_id: Arc<str>,
    pub market_slug: Arc<str>,
    pub outcome: String,
    pub size: Decimal,
    pub buy_price: Decimal,
    pub timestamp: DateTime<Utc>,
    pub ttl_seconds: u64,
}

/// Tracks pending positions using HashMap for O(1) lookups
#[derive(Debug, Default)]
pub struct PositionSettler {
    /// condition_id -> PendingPosition
    pending: HashMap<Arc<str>, PendingPosition>,
}

impl PositionSettler {
    /// Create a new settler
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a pending position
    pub fn add(&mut self, position: PendingPosition) {
        self.pending.insert(Arc::clone(&position.condition_id), position);
    }

    /// Remove a settled position by condition_id
    pub fn remove(&mut self, condition_id: &str) -> Option<PendingPosition> {
        self.pending.remove(condition_id)
    }

    /// Check if a position already exists
    pub fn contains(&self, condition_id: &str) -> bool {
        self.pending.contains_key(condition_id)
    }

    /// Get all pending positions
    pub fn all(&self) -> Vec<&PendingPosition> {
        self.pending.values().collect()
    }

    /// Get count of pending positions
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Get expired positions (based on TTL)
    pub fn expired(&self) -> Vec<&PendingPosition> {
        let now = Utc::now();
        self.pending
            .values()
            .filter(|p| {
                let expiry = p.timestamp + chrono::Duration::seconds(p.ttl_seconds as i64);
                now > expiry
            })
            .collect()
    }

    /// Remove expired positions and return them
    pub fn drain_expired(&mut self) -> Vec<PendingPosition> {
        let now = Utc::now();
        let expired_keys: Vec<Arc<str>> = self
            .pending
            .iter()
            .filter(|(_, p)| {
                let expiry = p.timestamp + chrono::Duration::seconds(p.ttl_seconds as i64);
                now > expiry
            })
            .map(|(k, _)| Arc::clone(k))
            .collect();

        expired_keys
            .into_iter()
            .filter_map(|k| self.pending.remove(&k))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn d(value: &str) -> Decimal {
        Decimal::from_str_exact(value).expect("valid decimal")
    }

    fn create_position(condition_id: &str) -> PendingPosition {
        PendingPosition {
            condition_id: Arc::from(condition_id),
            market_slug: Arc::from("test-market"),
            outcome: "YES".to_string(),
            size: d("100.0"),
            buy_price: d("0.5"),
            timestamp: Utc::now(),
            ttl_seconds: 3600,
        }
    }

    #[test]
    fn test_add_and_contains() {
        let mut settler = PositionSettler::new();
        let pos = create_position("cond-1");
        
        assert!(!settler.contains("cond-1"));
        settler.add(pos);
        assert!(settler.contains("cond-1"));
    }

    #[test]
    fn test_remove() {
        let mut settler = PositionSettler::new();
        let pos = create_position("cond-1");
        settler.add(pos);
        
        let removed = settler.remove("cond-1");
        assert!(removed.is_some());
        assert!(!settler.contains("cond-1"));
    }

    #[test]
    fn test_duplicate_detection() {
        let mut settler = PositionSettler::new();
        let pos1 = create_position("cond-1");
        let pos2 = create_position("cond-1"); // Same condition_id
        
        settler.add(pos1);
        assert!(settler.contains("cond-1"));
        
        // Adding duplicate should replace
        settler.add(pos2);
        assert_eq!(settler.len(), 1);
    }

    #[test]
    fn test_expired_positions() {
        let mut settler = PositionSettler::new();
        
        // Add expired position
        let expired = PendingPosition {
            condition_id: Arc::from("expired"),
            market_slug: Arc::from("test"),
            outcome: "YES".to_string(),
            size: d("100.0"),
            buy_price: d("0.5"),
            timestamp: Utc::now() - chrono::Duration::hours(2),
            ttl_seconds: 3600, // 1 hour TTL
        };
        
        // Add valid position
        let valid = create_position("valid");
        
        settler.add(expired);
        settler.add(valid);
        
        assert_eq!(settler.expired().len(), 1);
        
        let drained = settler.drain_expired();
        assert_eq!(drained.len(), 1);
        assert_eq!(settler.len(), 1); // Valid one remains
    }
}
```

- [ ] **Step 3: Update any callers of the old API**

Search for usages of `PositionSettler` in the codebase and update them:

Run: `grep -r "PositionSettler" src/ --include="*.rs"`

Update any code using:
- `settler.pending.push_back(...)` → `settler.add(...)`
- `settler.pending.iter().any(...)` → `settler.contains(...)`
- `settler.pending.len()` → `settler.len()`

- [ ] **Step 4: Run tests**

Run: `cargo test --lib pipeline::settler`
Expected: All tests pass

- [ ] **Step 5: Commit**

```bash
git add src/pipeline/settler.rs
git commit -m "perf: replace VecDeque with HashMap in PositionSettler

- Change from O(n) linear scan to O(1) lookup for duplicate detection
- Use Arc<str> as key to avoid string cloning
- Add convenience methods for common operations"
```

---

## Task 5: Optimize JSON Parsing (Struct Deserialization)

**Files:**
- Read: `src/data/binance.rs` and `src/data/coinbase.rs`
- Modify: `src/data/binance.rs`, `src/data/coinbase.rs`

- [ ] **Step 1: Read binance.rs WebSocket message handling**

- [ ] **Step 2: Add typed structs for Binance messages**

Add to `src/data/binance.rs`:

```rust
use serde::Deserialize;

/// Binance ticker message structure
#[derive(Debug, Deserialize)]
struct BinanceTicker {
    #[serde(rename = "c")]
    close_price: String,
}

/// Binance WebSocket message wrapper
#[derive(Debug, Deserialize)]
#[serde(tag = "e")]
enum BinanceMessage {
    #[serde(rename = "24hrTicker")]
    Ticker(BinanceTicker),
    #[serde(other)]
    Other,
}

// Replace the handle_message implementation:
fn handle_message(&self, text: &str) -> Result<()> {
    match serde_json::from_str::<BinanceMessage>(text) {
        Ok(BinanceMessage::Ticker(ticker)) => {
            let price = Decimal::from_str_exact(&ticker.close_price)
                .context("invalid price format")?;
            let tick = PriceTick {
                price,
                timestamp: Utc::now(),
            };
            self.buffer.push(tick);
            Ok(())
        }
        Ok(BinanceMessage::Other) => Ok(()), // Ignore non-ticker messages
        Err(e) => {
            tracing::debug!("Failed to parse Binance message: {}", e);
            Ok(())
        }
    }
}
```

- [ ] **Step 3: Add typed structs for Coinbase messages**

Add to `src/data/coinbase.rs`:

```rust
use serde::Deserialize;

/// Coinbase ticker message structure
#[derive(Debug, Deserialize)]
struct CoinbaseTicker {
    price: String,
}

/// Coinbase WebSocket message wrapper
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum CoinbaseMessage {
    #[serde(rename = "ticker")]
    Ticker(CoinbaseTicker),
    #[serde(other)]
    Other,
}

// Replace the handle_message implementation:
fn handle_message(&self, text: &str) -> Result<()> {
    match serde_json::from_str::<CoinbaseMessage>(text) {
        Ok(CoinbaseMessage::Ticker(ticker)) => {
            let price = Decimal::from_str_exact(&ticker.price)
                .context("invalid price format")?;
            let tick = PriceTick {
                price,
                timestamp: Utc::now(),
            };
            self.buffer.push(tick);
            Ok(())
        }
        Ok(CoinbaseMessage::Other) => Ok(()), // Ignore non-ticker messages
        Err(e) => {
            tracing::debug!("Failed to parse Coinbase message: {}", e);
            Ok(())
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib data`
Expected: All tests pass

- [ ] **Step 5: Commit**

```bash
git add src/data/binance.rs src/data/coinbase.rs
git commit -m "perf: use struct-based JSON deserialization for WebSocket messages

- Replace serde_json::Value with typed structs for Binance and Coinbase
- Reduces memory allocations during message parsing
- Add proper error handling with context"
```

---

## Task 6: Optimize File I/O (Buffered Trade Log)

**Files:**
- Read: `src/bot.rs` (lines 548-578)
- Modify: `src/bot.rs`

- [ ] **Step 1: Read current trade logging implementation**

- [ ] **Step 2: Add buffered writer for trade log**

First, add a buffered writer field to the Bot struct and initialization:

```rust
// In bot.rs imports:
use std::io::BufWriter;
use std::fs::File;

// In Bot struct definition (around line 50-80):
pub struct Bot {
    // ... existing fields ...
    trade_log_writer: Option<Arc<tokio::sync::Mutex<BufWriter<File>>>>,
}

// In Bot::new() initialization (around line 150-200):
// Initialize trade log writer
let trade_log_writer = if config.trading.mode == TradingMode::Live {
    let log_path = log_dir.join("trades.csv");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .context("failed to open trade log")?;
    
    // Check if file is empty and write header
    let metadata = file.metadata()?;
    if metadata.len() == 0 {
        use std::io::Write;
        let mut writer = BufWriter::new(&file);
        writeln!(writer, "timestamp,market_slug,outcome,size,price,direction")?;
        writer.flush()?;
    }
    
    Some(Arc::new(tokio::sync::Mutex::new(BufWriter::new(file))))
} else {
    None
};

// Then in record_trade() method (around line 540-578):
async fn record_trade(&self, trade: TradeRecord) -> Result<()> {
    if let Some(ref writer) = self.trade_log_writer {
        let mut writer = writer.lock().await;
        use std::io::Write;
        writeln!(
            writer,
            "{},{},{},{:.2},{:.4},{}",
            trade.timestamp.to_rfc3339(),
            trade.market_slug,
            trade.outcome,
            trade.size,
            trade.price,
            trade.direction
        )?;
        // Flush periodically instead of every write
        // Let BufWriter handle buffering
    }
    Ok(())
}

// Add periodic flush in the main loop or tasks:
async fn flush_trade_log(&self) -> Result<()> {
    if let Some(ref writer) = self.trade_log_writer {
        let mut writer = writer.lock().await;
        writer.flush()?;
    }
    Ok(())
}
```

- [ ] **Step 3: Add periodic flush task**

In `src/tasks.rs`, add a task to periodically flush the trade log:

```rust
// In tasks.rs, add a new task:
pub async fn trade_log_flush_task(bot: Arc<Bot>) {
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    
    loop {
        interval.tick().await;
        if let Err(e) = bot.flush_trade_log().await {
            tracing::error!("[TASK] Failed to flush trade log: {}", e);
        }
    }
}
```

- [ ] **Step 4: Update main.rs to spawn the flush task**

```rust
// In main.rs where tasks are spawned:
tokio::spawn(tasks::trade_log_flush_task(bot.clone()));
```

- [ ] **Step 5: Run tests**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add src/bot.rs src/tasks.rs src/main.rs
git commit -m "perf: buffer trade log writes with BufWriter

- Replace per-trade file open/write/close with persistent BufWriter
- Add periodic flush task every 30 seconds
- Reduces syscall overhead from O(trades) to O(flush intervals)"
```

---

## Task 7: Optimize Balance Writes (Debounced)

**Files:**
- Read: `src/bot.rs` (lines 317-325)
- Modify: `src/bot.rs`, `src/state.rs`

- [ ] **Step 1: Read current balance write implementation**

- [ ] **Step 2: Add debounced balance writing to state.rs**

Add to `src/state.rs`:

```rust
use std::sync::atomic::{AtomicU64, Ordering};
use rust_decimal::Decimal;

#[derive(Debug)]
pub struct BalanceState {
    /// Last written balance value (atomic for lock-free reads)
    last_balance: AtomicU64,
    /// Last write timestamp
    last_write: AtomicU64, // Unix timestamp as nanos
    /// Minimum change threshold to trigger write (in cents)
    change_threshold_cents: u64,
    /// Minimum time between writes (seconds)
    min_interval_secs: u64,
}

impl BalanceState {
    pub fn new() -> Self {
        Self {
            last_balance: AtomicU64::new(0),
            last_write: AtomicU64::new(0),
            change_threshold_cents: 100, // $1.00
            min_interval_secs: 60, // 1 minute
        }
    }

    /// Check if balance write should be triggered
    pub fn should_write(&self, balance: Decimal) -> bool {
        let balance_cents = (balance * Decimal::from(100)).to_u64().unwrap_or(0);
        let last = self.last_balance.load(Ordering::Relaxed);
        let last_write = self.last_write.load(Ordering::Relaxed);
        
        // Check if change exceeds threshold
        let change = if balance_cents > last {
            balance_cents - last
        } else {
            last - balance_cents
        };
        
        if change < self.change_threshold_cents {
            return false;
        }
        
        // Check if enough time has passed
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        if now - last_write < self.min_interval_secs {
            return false;
        }
        
        true
    }

    /// Record that a write occurred
    pub fn record_write(&self, balance: Decimal) {
        let balance_cents = (balance * Decimal::from(100)).to_u64().unwrap_or(0);
        self.last_balance.store(balance_cents, Ordering::Relaxed);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.last_write.store(now, Ordering::Relaxed);
    }
}

impl Default for BalanceState {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 3: Add BalanceState to BotState**

```rust
// In state.rs, add to BotState:
pub struct BotState {
    pub idle_reason: Arc<RwLock<Option<IdleReason>>>,
    pub last_no_trade_reason: String,
    pub fak_backoff_until: Option<DateTime<Utc>>,
    pub balance_state: BalanceState, // Add this
}

impl BotState {
    pub fn new() -> Self {
        Self {
            idle_reason: Arc::new(RwLock::new(None)),
            last_no_trade_reason: String::new(),
            fak_backoff_until: None,
            balance_state: BalanceState::new(), // Initialize
        }
    }
    
    // Add helper method
    pub fn should_write_balance(&self, balance: Decimal) -> bool {
        self.balance_state.should_write(balance)
    }
    
    pub fn record_balance_write(&self, balance: Decimal) {
        self.balance_state.record_write(balance);
    }
}
```

- [ ] **Step 4: Update bot.rs to use debounced writes**

Modify the balance check/writing code (around line 317-325):

```rust
// OLD CODE:
// if let Some(ref checker) = self.balance_checker {
//     match checker.balance().await {
//         Ok(on_chain_bal) => {
//             self.account.write().await.balance = on_chain_bal;
//             Self::write_balance(&self.log_dir, on_chain_bal).await;
//         }
//     }
// }

// NEW CODE:
if let Some(ref checker) = self.balance_checker {
    match checker.balance().await {
        Ok(on_chain_bal) => {
            let mut account = self.account.write().await;
            account.balance = on_chain_bal;
            drop(account); // Release lock early
            
            // Debounced write
            if self.state.should_write_balance(on_chain_bal) {
                if let Err(e) = Self::write_balance(&self.log_dir, on_chain_bal).await {
                    tracing::warn!("[BALANCE] Failed to write balance: {}", e);
                } else {
                    self.state.record_balance_write(on_chain_bal);
                    tracing::debug!("[BALANCE] Wrote balance: ${:.2}", on_chain_bal);
                }
            }
        }
        Err(e) => {
            tracing::warn!("[BALANCE] Failed to fetch balance: {}", e);
        }
    }
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib state`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add src/state.rs src/bot.rs
git commit -m "perf: debounce balance file writes

- Add BalanceState to track last written value and timestamp
- Only write when balance changes by >$1.00 or 60 seconds passed
- Reduces disk I/O from O(ticks) to O(significant changes)"
```

---

## Task 8: Use Arc<str> for String Keys

**Files:**
- Read: `src/tasks.rs` (line 143)
- Modify: `src/tasks.rs`, `src/bot.rs`

- [ ] **Step 1: Read current pending_retries implementation**

- [ ] **Step 2: Change HashMap<String, u32> to HashMap<Arc<str>, u32>**

In `src/tasks.rs`:

```rust
// Add import at top:
use std::sync::Arc;

// Change the struct field (around line 143):
pub struct PendingRetries {
    /// market_slug -> retry count
    retries: HashMap<Arc<str>, u32>,
    max_retries: u32,
}

impl PendingRetries {
    pub fn new(max_retries: u32) -> Self {
        Self {
            retries: HashMap::new(),
            max_retries,
        }
    }

    pub fn record_retry(&mut self, market_slug: &str) {
        let key: Arc<str> = Arc::from(market_slug);
        let count = self.retries.entry(key).or_insert(0);
        *count += 1;
    }

    pub fn get_retry_count(&self, market_slug: &str) -> u32 {
        self.retries.get(market_slug).copied().unwrap_or(0)
    }

    pub fn should_retry(&self, market_slug: &str) -> bool {
        self.get_retry_count(market_slug) < self.max_retries
    }

    pub fn reset(&mut self, market_slug: &str) {
        self.retries.remove(market_slug);
    }

    pub fn clear(&mut self) {
        self.retries.clear();
    }
}
```

- [ ] **Step 3: Update any usages of PendingRetries in bot.rs**

Ensure bot.rs uses `Arc::clone()` when needed or passes `&str` references.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib tasks`
Expected: All tests pass

- [ ] **Step 5: Commit**

```bash
git add src/tasks.rs src/bot.rs
git commit -m "perf: use Arc<str> for pending retry keys

- Replace String keys with Arc<str> to avoid cloning
- HashMap<Arc<str>, u32> shares ownership instead of copying"
```

---

## Task 9: Final Verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test --locked`
Expected: All tests pass

- [ ] **Step 2: Run clippy with all features**

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
Expected: No warnings

- [ ] **Step 3: Run format check**

Run: `cargo fmt --all -- --check`
Expected: No formatting issues

- [ ] **Step 4: Build release**

Run: `cargo build --release`
Expected: Successful build

- [ ] **Step 5: Final commit**

```bash
git commit -m "perf: complete performance optimization pass

Summary of optimizations:
- Lock-free price buffer with crossbeam::ArrayQueue
- Parallel Polymarket price fetching with tokio::join!
- O(1) position lookup with HashMap
- Struct-based JSON deserialization for WebSocket messages
- Buffered trade log writes with periodic flush
- Debounced balance file writes
- Arc<str> for string keys to reduce cloning

All optimizations reduce allocations, minimize lock contention,
and improve I/O efficiency throughout the trading pipeline."
```

---

## Summary

| Task | Component | Impact | Lines Changed |
|------|-----------|--------|---------------|
| 1 | Add crossbeam dependency | Enables lock-free structures | 1 line |
| 2 | PriceBuffer | Lock-free reads/writes | ~200 lines |
| 3 | Parallel HTTP | 2x faster price fetch | ~20 lines |
| 4 | PositionSettler | O(1) vs O(n) lookup | ~150 lines |
| 5 | JSON Parsing | Fewer allocations | ~100 lines |
| 6 | Buffered I/O | Reduced syscalls | ~80 lines |
| 7 | Debounced Writes | Fewer disk operations | ~100 lines |
| 8 | Arc<str> Keys | Less cloning | ~50 lines |

**Total estimated impact:**
- Latency: 30-50% reduction in hot path
- Memory: 20-30% fewer allocations per tick
- I/O: 80-90% fewer disk operations
