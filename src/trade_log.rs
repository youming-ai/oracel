//! Shared trades.csv writer — single owner of the file handle.
//!
//! Eliminates the race condition of two concurrent writers (tick entry +
//! settlement checker) by centralizing all writes through one
//! `Arc<Mutex<BufWriter<File>>`>.

use std::io::{BufWriter, Write};
use std::path::Path;

use chrono::Utc;
use rust_decimal::Decimal;

use crate::pipeline::decider::{estimate_probability, DecideContext, Decision};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct TradeLog {
    writer: Arc<Mutex<BufWriter<std::fs::File>>>,
    observations: Arc<Mutex<BufWriter<std::fs::File>>>,
    outcomes: Arc<Mutex<BufWriter<std::fs::File>>>,
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
        let observations_path = Path::new(log_dir).join("observations.csv");
        let observations_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&observations_path)?;
        let observations_metadata = observations_file.metadata()?;
        let mut observations = BufWriter::new(observations_file);
        if observations_metadata.len() == 0 {
            writeln!(
                observations,
                "timestamp,market_slug,remaining_ms,chainlink_open,chainlink_current,binance_price,sigma_per_second,normalized_move,model_p_up,trend_15s_pct,trend_30s_pct,up_bid,up_ask,up_effective,up_depth,down_bid,down_ask,down_effective,down_depth,decision,direction,selected_probability,net_edge"
            )?;
        }
        let outcomes_path = Path::new(log_dir).join("outcomes.csv");
        let outcomes_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&outcomes_path)?;
        let outcomes_metadata = outcomes_file.metadata()?;
        let mut outcomes = BufWriter::new(outcomes_file);
        if outcomes_metadata.len() == 0 {
            writeln!(
                outcomes,
                "timestamp,market_slug,result,direction,pnl,entry_chainlink,chainlink_at_settlement_check"
            )?;
        }
        Ok(Self {
            writer: Arc::new(Mutex::new(writer)),
            observations: Arc::new(Mutex::new(observations)),
            outcomes: Arc::new(Mutex::new(outcomes)),
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

    /// Record every model evaluation, including passes, for offline calibration.
    pub async fn log_observation(
        &self,
        market_slug: &str,
        binance_price: Decimal,
        ctx: &DecideContext,
        decision: &Decision,
    ) {
        let quote_value = |quote: Option<crate::data::polymarket::BuyQuote>, field: &str| {
            quote
                .and_then(|quote| match field {
                    "bid" => quote.best_bid,
                    "ask" => Some(quote.best_ask),
                    "effective" => Some(quote.effective_price),
                    "depth" => Some(quote.best_ask_notional),
                    _ => None,
                })
                .map(|value| value.round_dp(8).to_string())
                .unwrap_or_default()
        };
        let (reason, direction, probability, edge) = match decision {
            Decision::Pass(reason) => (reason.as_str(), "", String::new(), String::new()),
            Decision::Trade {
                direction,
                model_probability,
                edge,
                ..
            } => (
                "trade",
                direction.as_str(),
                model_probability.round_dp(8).to_string(),
                edge.round_dp(8).to_string(),
            ),
        };
        let decimal = |value: Option<Decimal>| {
            value
                .map(|value| value.round_dp(8).to_string())
                .unwrap_or_default()
        };
        let estimate = estimate_probability(ctx);
        let line = format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ"),
            market_slug,
            ctx.remaining_ms,
            decimal(ctx.chainlink_open),
            decimal(ctx.chainlink_current),
            binance_price.round_dp(8),
            decimal(ctx.sigma_per_second),
            decimal(estimate.map(|estimate| estimate.normalized_move)),
            decimal(estimate.map(|estimate| estimate.p_up)),
            decimal(ctx.binance_trend_15s_pct),
            decimal(ctx.binance_trend_30s_pct),
            quote_value(ctx.up_quote, "bid"),
            quote_value(ctx.up_quote, "ask"),
            quote_value(ctx.up_quote, "effective"),
            quote_value(ctx.up_quote, "depth"),
            quote_value(ctx.down_quote, "bid"),
            quote_value(ctx.down_quote, "ask"),
            quote_value(ctx.down_quote, "effective"),
            quote_value(ctx.down_quote, "depth"),
            reason,
            direction,
            probability,
            edge,
        );
        let mut writer = self.observations.lock().await;
        if let Err(error) = writer.write_all(line.as_bytes()) {
            tracing::warn!("[LOG] observations.csv write failed: {}", error);
        }
    }

    /// Flush buffered writes to disk.
    pub async fn flush(&self) {
        let mut writer = self.writer.lock().await;
        if let Err(error) = writer.flush() {
            tracing::warn!("[LOG] trades.csv flush failed: {}", error);
        }
        drop(writer);
        let mut observations = self.observations.lock().await;
        if let Err(error) = observations.flush() {
            tracing::warn!("[LOG] observations.csv flush failed: {}", error);
        }
        drop(observations);
        let mut outcomes = self.outcomes.lock().await;
        if let Err(error) = outcomes.flush() {
            tracing::warn!("[LOG] outcomes.csv flush failed: {}", error);
        }
    }

    pub fn clone_handle(&self) -> TradeLogHandle {
        TradeLogHandle {
            writer: self.writer.clone(),
            outcomes: self.outcomes.clone(),
        }
    }
}

/// Cheap cloneable handle for passing to background tasks.
#[derive(Clone)]
pub struct TradeLogHandle {
    writer: Arc<Mutex<BufWriter<std::fs::File>>>,
    outcomes: Arc<Mutex<BufWriter<std::fs::File>>>,
}

impl TradeLogHandle {
    /// Log a settlement result (called from settlement checker).
    pub async fn log_settlement(
        &self,
        market_slug: &str,
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
        drop(w);

        let outcome_line = format!(
            "{},{},{},{},{},{},{}\n",
            Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
            market_slug,
            result,
            direction,
            pnl.round_dp(8),
            entry_btc_price.round_dp(8),
            current_btc_price.round_dp(8),
        );
        let mut outcomes = self.outcomes.lock().await;
        if let Err(error) = outcomes.write_all(outcome_line.as_bytes()) {
            tracing::warn!("[LOG] outcomes.csv settlement write failed: {}", error);
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
        log.clone_handle()
            .log_settlement(
                "btc-updown-5m-test",
                true,
                "UP",
                d("20.0"),
                d("70000"),
                d("70500"),
            )
            .await;
        log.flush().await;

        let content = std::fs::read_to_string(dir.path().join("trades.csv")).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert!(lines[1].contains("ENTRY,UP,abc12345"));
        assert!(lines[2].contains("WIN"));
        assert!(lines[2].contains("UP"));
        let outcomes = std::fs::read_to_string(dir.path().join("outcomes.csv")).unwrap();
        assert!(outcomes.contains("btc-updown-5m-test,WIN,UP"));
    }

    #[tokio::test]
    async fn test_handle_logs_settlement() {
        let dir = tempfile::tempdir().unwrap();
        let log = TradeLog::open(dir.path().to_str().unwrap()).unwrap();
        let handle = log.clone_handle();
        handle
            .log_settlement(
                "btc-updown-5m-test",
                false,
                "DOWN",
                d("-5.0"),
                d("70000"),
                d("69500"),
            )
            .await;
        log.flush().await;

        let content = std::fs::read_to_string(dir.path().join("trades.csv")).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert!(lines[1].contains("LOSS,DOWN"));
    }
}
