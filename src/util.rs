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
