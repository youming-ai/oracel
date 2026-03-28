use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

/// Tracks balance writes with debouncing to reduce disk I/O
#[derive(Debug)]
pub(crate) struct BalanceState {
    /// Last written balance value in cents (atomic for lock-free reads)
    last_balance_cents: AtomicU64,
    /// Last write timestamp (Unix seconds)
    last_write_secs: AtomicU64,
    /// Minimum change threshold to trigger write (in cents)
    change_threshold_cents: u64,
    /// Minimum time between writes (seconds)
    min_interval_secs: u64,
}

impl BalanceState {
    pub(crate) fn new() -> Self {
        Self {
            last_balance_cents: AtomicU64::new(0),
            last_write_secs: AtomicU64::new(0),
            change_threshold_cents: 100, // $1.00
            min_interval_secs: 60,       // 1 minute
        }
    }

    /// Check if balance write should be triggered
    pub(crate) fn should_write(&self, balance: Decimal) -> bool {
        let balance_cents = (balance * Decimal::from(100)).to_u64().unwrap_or(0);
        let last = self.last_balance_cents.load(Ordering::Relaxed);
        let last_write = self.last_write_secs.load(Ordering::Relaxed);

        // Check if change exceeds threshold
        let change = balance_cents.abs_diff(last);

        if change < self.change_threshold_cents {
            return false;
        }

        // Check if enough time has passed
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if now.saturating_sub(last_write) < self.min_interval_secs {
            return false;
        }

        true
    }

    /// Record that a write occurred
    pub(crate) fn record_write(&self, balance: Decimal) {
        let balance_cents = (balance * Decimal::from(100)).to_u64().unwrap_or(0);
        self.last_balance_cents
            .store(balance_cents, Ordering::Relaxed);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.last_write_secs.store(now, Ordering::Relaxed);
    }
}

impl Default for BalanceState {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) struct BotState {
    pub last_no_trade_reason: String,
    pub last_idle_reason: String,
    pub fak_rejections: u32,
    pub fak_market_ms: i64,
    pub last_fak_rejection_ms: i64,
    pub balance_state: BalanceState,
}

impl BotState {
    pub(crate) fn new() -> Self {
        Self {
            last_no_trade_reason: String::new(),
            last_idle_reason: String::new(),
            fak_rejections: 0,
            fak_market_ms: 0,
            last_fak_rejection_ms: 0,
            balance_state: BalanceState::new(),
        }
    }

    pub(crate) fn log_idle_change(&mut self, reason: &str, detail: &str) {
        if self.last_idle_reason != reason {
            self.last_idle_reason = reason.to_string();
            tracing::info!("[IDLE] {} | {}", reason, detail);
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct MarketState {
    pub token_yes: Arc<str>,
    pub token_no: Arc<str>,
    pub condition_id: Arc<str>,
    pub market_slug: Arc<str>,
    pub settlement_ms: i64,
}

impl MarketState {
    pub(crate) fn is_ready(&self) -> bool {
        !self.token_yes.is_empty() && !self.token_no.is_empty()
    }
}
