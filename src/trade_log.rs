//! Shared trades.csv writer — single owner of the file handle.
//!
//! Eliminates the race condition of two concurrent writers (tick entry +
//! settlement checker) by centralizing all writes through one
//! `Arc<Mutex<BufWriter<File>>`>.

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
    #[allow(clippy::too_many_arguments)]
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
        let id_short = &order_id[..8.min(order_id.len())];
        let line = format!(
            "{},ENTRY,{},{},{},{},{},{},{}s,{},{},{}x\n",
            Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
            direction,
            id_short,
            entry_price.round_dp(3),
            cost.round_dp(2),
            edge.round_dp(1),
            balance.round_dp(2),
            remaining_ms / 1000,
            yes_price.unwrap_or_default().round_dp(3),
            no_price.unwrap_or_default().round_dp(3),
            payoff_ratio.round_dp(1),
        );
        let mut w = self.writer.lock().await;
        if let Err(e) = w.write_all(line.as_bytes()) {
            tracing::warn!("[LOG] trades.csv entry write failed: {}", e);
        }
    }

    /// Log a settlement result (delegates to handle).
    pub async fn log_settlement(
        &self,
        won: bool,
        direction: &str,
        pnl: Decimal,
        entry_btc_price: Decimal,
        current_btc_price: Decimal,
    ) {
        self.clone_handle()
            .log_settlement(won, direction, pnl, entry_btc_price, current_btc_price)
            .await;
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
        let pnl_str = format!("{:+.2}", pnl.round_dp(2));
        let line = format!(
            "{},{},{},{},{},{},{}\n",
            Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
            result,
            direction,
            "", // no order_id for settlement
            pnl_str,
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

    fn d(s: &str) -> Decimal {
        Decimal::from_str_exact(s).expect("valid decimal")
    }

    #[test]
    fn test_trade_log_writes_header_on_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let log = TradeLog::open(dir.path().to_str().unwrap()).unwrap();
        log.writer.blocking_lock().flush().unwrap();

        let content = std::fs::read_to_string(dir.path().join("trades.csv")).unwrap();
        assert!(content.contains("timestamp,type,direction"));
        assert!(!content.contains("ENTRY")); // header only
    }

    #[test]
    fn test_trade_log_appends_without_header() {
        let dir = tempfile::tempdir().unwrap();
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
            "UP",
            "abc1234567",
            d("0.05"),
            d("5.00"),
            d("45.0"),
            d("95.00"),
            180000,
            Some(d("0.95")),
            Some(d("0.05")),
            d("19.0"),
        )
        .await;
        log.log_settlement(true, "UP", d("20.0"), d("70000"), d("70500"))
            .await;
        log.flush().await;

        let content = std::fs::read_to_string(dir.path().join("trades.csv")).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert!(lines[1].contains("ENTRY,UP,abc12345"));
        assert!(lines[2].contains("WIN"));
        assert!(lines[2].contains("UP"));
    }

    #[tokio::test]
    async fn test_handle_logs_settlement() {
        let dir = tempfile::tempdir().unwrap();
        let log = TradeLog::open(dir.path().to_str().unwrap()).unwrap();
        let handle = log.clone_handle();
        handle
            .log_settlement(false, "DOWN", d("-5.0"), d("70000"), d("69500"))
            .await;
        log.flush().await;

        let content = std::fs::read_to_string(dir.path().join("trades.csv")).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert!(lines[1].contains("LOSS,DOWN"));
    }
}
