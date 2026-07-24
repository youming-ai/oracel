//! Shared helpers.

use rust_decimal::Decimal;
use std::path::Path;

pub fn decimal(value: &'static str) -> Decimal {
    Decimal::from_str_exact(value).expect(value)
}

/// Atomically write balance to file (write tmp, then rename).
pub async fn write_balance(log_dir: &str, bal: Decimal) {
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

/// Name of the durability marker written when a risk/position state write fails.
/// Its presence at startup means the previous run may hold unpersisted state, so
/// entries stay halted until an operator resolves it.
const STATE_WRITE_FAILED_MARKER: &str = "state_write_failed";

/// Best-effort marker that a state write failed. Failure to write the marker is
/// itself only logged: the in-memory halt still protects the running process.
pub async fn mark_state_write_failed(log_dir: &str) {
    let path = Path::new(log_dir).join(STATE_WRITE_FAILED_MARKER);
    if let Err(e) = tokio::fs::write(&path, b"state write failed; entries halted\n").await {
        tracing::error!("[STATE] failed to write durability marker: {e}");
    }
}

/// Whether a prior run left a durability marker behind.
pub fn state_write_failed(log_dir: &str) -> bool {
    Path::new(log_dir).join(STATE_WRITE_FAILED_MARKER).exists()
}

/// React to a failed risk/position state write: set the sticky halt flag (block
/// new entries) and record the durability marker. Centralizes the failure
/// response so every persist site behaves identically.
pub async fn halt_on_state_write_failure(
    persist_halt: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    log_dir: &str,
) {
    persist_halt.store(true, std::sync::atomic::Ordering::Release);
    mark_state_write_failed(log_dir).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn durability_marker_round_trips() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().to_str().unwrap();
        assert!(!state_write_failed(path));
        mark_state_write_failed(path).await;
        assert!(state_write_failed(path));
    }

    #[tokio::test]
    async fn halt_on_failure_sets_flag_and_marker() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().to_str().unwrap();
        let halt = Arc::new(AtomicBool::new(false));
        halt_on_state_write_failure(&halt, path).await;
        assert!(halt.load(Ordering::Acquire));
        assert!(state_write_failed(path));
    }
}
