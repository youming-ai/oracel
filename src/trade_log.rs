//! Mode-isolated CSV logs for Binance Prediction trades and model calibration.

use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use rust_decimal::Decimal;
use tokio::sync::Mutex;

use crate::data::binance_prediction::{ActivePredictionMarket, BuyQuote};
use crate::pipeline::decider::{estimate_probability, DecideContext, Decision};
use crate::pipeline::executor::OrderResult;
use crate::pipeline::settler::{PendingPosition, SettlementResult};

pub struct TradeLog {
    trades: Arc<Mutex<BufWriter<std::fs::File>>>,
    observations: Arc<Mutex<BufWriter<std::fs::File>>>,
    outcomes: Arc<Mutex<BufWriter<std::fs::File>>>,
}

impl TradeLog {
    pub fn open(log_dir: &str) -> std::io::Result<Self> {
        let trades = open_csv(
            Path::new(log_dir).join("trades.csv"),
            "timestamp,type,market_topic_id,market_slug,direction,order_id,entry_price,trade_cost,fee,total_cost,edge,balance,remaining_ms,up_price,down_price,payoff_ratio\n",
        )?;
        let observations = open_csv(
            Path::new(log_dir).join("observations.csv"),
            "timestamp,market_topic_id,market_slug,remaining_ms,reference_price,binance_price,sigma_per_second,normalized_move,model_p_up,trend_15s_pct,trend_30s_pct,up_bid,up_ask,up_effective,up_depth,down_bid,down_ask,down_effective,down_depth,decision,direction,selected_probability,net_edge\n",
        )?;
        let outcomes = open_csv(
            Path::new(log_dir).join("outcomes.csv"),
            "timestamp,market_topic_id,market_slug,result,direction,pnl,entry_binance_price,binance_at_settlement_check\n",
        )?;
        Ok(Self {
            trades: Arc::new(Mutex::new(trades)),
            observations: Arc::new(Mutex::new(observations)),
            outcomes: Arc::new(Mutex::new(outcomes)),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn log_entry(
        &self,
        market: &ActivePredictionMarket,
        order: &OrderResult,
        edge: Decimal,
        balance: Decimal,
        remaining_ms: i64,
        up_quote: Option<BuyQuote>,
        down_quote: Option<BuyQuote>,
        payoff_ratio: Decimal,
    ) {
        let line = entry_line(
            market,
            order,
            edge,
            balance,
            remaining_ms,
            up_quote,
            down_quote,
            payoff_ratio,
        );
        write_line(&self.trades, line, "trades.csv entry").await;
    }

    pub async fn log_observation(
        &self,
        market: &ActivePredictionMarket,
        binance_price: Decimal,
        context: &DecideContext,
        decision: &Decision,
    ) {
        let quote_value = |quote: Option<BuyQuote>, field: &str| -> String {
            quote
                .and_then(|quote| match field {
                    "bid" => quote.best_bid,
                    "ask" => Some(quote.best_ask),
                    "effective" => Some(quote.effective_price),
                    "depth" => Some(quote.best_ask_notional),
                    _ => None,
                })
                .map(decimal)
                .unwrap_or_default()
        };
        let estimate = estimate_probability(context);
        let (decision_name, direction, selected_probability, edge) = match decision {
            Decision::Pass(reason) => (reason.as_str(), "", String::new(), String::new()),
            Decision::Trade {
                direction,
                model_probability,
                edge,
                ..
            } => (
                "trade",
                direction.as_str(),
                decimal(*model_probability),
                decimal(*edge),
            ),
        };
        let value = |value: Option<Decimal>| value.map(decimal).unwrap_or_default();
        let line = format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            timestamp(),
            market.market_topic_id,
            csv(&market.slug),
            context.remaining_ms,
            value(context.reference_price),
            decimal(binance_price),
            value(context.sigma_per_second),
            value(estimate.map(|estimate| estimate.normalized_move)),
            value(estimate.map(|estimate| estimate.p_up)),
            value(context.binance_trend_15s_pct),
            value(context.binance_trend_30s_pct),
            quote_value(context.up_quote, "bid"),
            quote_value(context.up_quote, "ask"),
            quote_value(context.up_quote, "effective"),
            quote_value(context.up_quote, "depth"),
            quote_value(context.down_quote, "bid"),
            quote_value(context.down_quote, "ask"),
            quote_value(context.down_quote, "effective"),
            quote_value(context.down_quote, "depth"),
            csv(decision_name),
            direction,
            selected_probability,
            edge,
        );
        write_line(&self.observations, line, "observations.csv").await;
    }

    pub async fn flush(&self) {
        flush_writer(&self.trades, "trades.csv").await;
        flush_writer(&self.observations, "observations.csv").await;
        flush_writer(&self.outcomes, "outcomes.csv").await;
    }

    pub fn clone_handle(&self) -> TradeLogHandle {
        TradeLogHandle {
            trades: Arc::clone(&self.trades),
            outcomes: Arc::clone(&self.outcomes),
        }
    }
}

#[derive(Clone)]
pub struct TradeLogHandle {
    trades: Arc<Mutex<BufWriter<std::fs::File>>>,
    outcomes: Arc<Mutex<BufWriter<std::fs::File>>>,
}

impl TradeLogHandle {
    #[allow(clippy::too_many_arguments)]
    pub async fn log_entry(
        &self,
        market: &ActivePredictionMarket,
        order: &OrderResult,
        edge: Decimal,
        balance: Decimal,
        remaining_ms: i64,
        up_quote: Option<BuyQuote>,
        down_quote: Option<BuyQuote>,
        payoff_ratio: Decimal,
    ) {
        write_line(
            &self.trades,
            entry_line(
                market,
                order,
                edge,
                balance,
                remaining_ms,
                up_quote,
                down_quote,
                payoff_ratio,
            ),
            "trades.csv reconciled entry",
        )
        .await;
    }

    pub async fn log_reconciled_entry(
        &self,
        position: &PendingPosition,
        order: &OrderResult,
        balance: Decimal,
    ) {
        let line = format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            timestamp(),
            "ENTRY",
            position.market_topic_id,
            csv(&position.market_slug),
            order.direction.as_str(),
            csv(&order.order_id),
            decimal(order.entry_price),
            decimal(order.trade_cost),
            decimal(order.fee),
            decimal(order.total_cost()),
            "",
            decimal(balance),
            "",
            "",
            "",
            "",
        );
        write_line(&self.trades, line, "trades.csv reconciled entry").await;
    }

    pub async fn log_settlement(&self, result: &SettlementResult, current_binance_price: Decimal) {
        let outcome = if result.won { "WIN" } else { "LOSS" };
        let trade_line = format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            timestamp(),
            outcome,
            result.market_topic_id,
            csv(&result.market_slug),
            result.direction.as_str(),
            "",
            "",
            "",
            "",
            "",
            decimal(result.pnl),
            "",
            "",
            "",
            "",
            "",
        );
        write_line(&self.trades, trade_line, "trades.csv settlement").await;
        let outcome_line = format!(
            "{},{},{},{},{},{},{},{}\n",
            timestamp(),
            result.market_topic_id,
            csv(&result.market_slug),
            outcome,
            result.direction.as_str(),
            decimal(result.pnl),
            decimal(result.entry_btc_price),
            decimal(current_binance_price),
        );
        write_line(&self.outcomes, outcome_line, "outcomes.csv").await;
    }
}

fn open_csv(path: std::path::PathBuf, header: &str) -> std::io::Result<BufWriter<std::fs::File>> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let is_new = file.metadata()?.len() == 0;
    let mut writer = BufWriter::new(file);
    if is_new {
        writer.write_all(header.as_bytes())?;
    }
    Ok(writer)
}

#[allow(clippy::too_many_arguments)]
fn entry_line(
    market: &ActivePredictionMarket,
    order: &OrderResult,
    edge: Decimal,
    balance: Decimal,
    remaining_ms: i64,
    up_quote: Option<BuyQuote>,
    down_quote: Option<BuyQuote>,
    payoff_ratio: Decimal,
) -> String {
    format!(
        "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
        timestamp(),
        "ENTRY",
        market.market_topic_id,
        csv(&market.slug),
        order.direction.as_str(),
        csv(&order.order_id),
        decimal(order.entry_price),
        decimal(order.trade_cost),
        decimal(order.fee),
        decimal(order.total_cost()),
        decimal(edge * Decimal::from(100)),
        decimal(balance),
        remaining_ms,
        up_quote
            .map(|quote| decimal(quote.effective_price))
            .unwrap_or_default(),
        down_quote
            .map(|quote| decimal(quote.effective_price))
            .unwrap_or_default(),
        decimal(payoff_ratio),
    )
}

async fn write_line(writer: &Arc<Mutex<BufWriter<std::fs::File>>>, line: String, label: &str) {
    let mut writer = writer.lock().await;
    if let Err(error) = writer.write_all(line.as_bytes()) {
        tracing::warn!("[LOG] {label} write failed: {error}");
    }
}

async fn flush_writer(writer: &Arc<Mutex<BufWriter<std::fs::File>>>, label: &str) {
    let mut writer = writer.lock().await;
    if let Err(error) = writer.flush() {
        tracing::warn!("[LOG] {label} flush failed: {error}");
    }
}

fn timestamp() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

fn decimal(value: Decimal) -> String {
    value.round_dp(8).normalize().to_string()
}

fn csv(value: &str) -> String {
    // The TUI loader parses these files with a naive `split(',')`, so a field must
    // never contain a separator. Binance slugs/IDs shouldn't anyway; strip
    // defensively rather than emit quoted fields the reader can't understand.
    if value.contains([',', '"', '\n', '\r']) {
        value.replace([',', '"', '\n', '\r'], "_")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::binance_prediction::MarketToken;
    use crate::pipeline::decider::Direction;
    use crate::pipeline::test_helpers::d;

    fn market() -> ActivePredictionMarket {
        ActivePredictionMarket {
            market_topic_id: 7,
            vendor: "PREDICT_FUN".into(),
            slug: "btc-5m-7".into(),
            title: "BTC".into(),
            start_ms: 0,
            end_ms: 300_000,
            reference_price: d("64000"),
            fee_rate_bps: 200,
            up: MarketToken {
                market_id: 42,
                token_id: "up".into(),
            },
            down: MarketToken {
                market_id: 42,
                token_id: "down".into(),
            },
        }
    }

    fn order() -> OrderResult {
        OrderResult {
            order_id: "order-1".into(),
            direction: Direction::Up,
            requested_usdt: d("2"),
            entry_price: d("0.5"),
            filled_shares: d("4"),
            trade_cost: d("2"),
            fee: d("0.04"),
            settlement_time_ms: 300_000,
            entry_btc_price: d("64000"),
        }
    }

    #[tokio::test]
    async fn writes_binance_entry_and_outcome_rows() {
        let directory = tempfile::tempdir().unwrap();
        let log = TradeLog::open(directory.path().to_str().unwrap()).unwrap();
        log.log_entry(
            &market(),
            &order(),
            d("0.10"),
            d("97.96"),
            120_000,
            None,
            None,
            d("1"),
        )
        .await;
        let result = SettlementResult {
            market_topic_id: 7,
            market_slug: "btc-5m-7".into(),
            token_id: "up".into(),
            direction: Direction::Up,
            payout: d("4"),
            pnl: d("1.96"),
            won: true,
            entry_btc_price: d("64000"),
        };
        log.clone_handle().log_settlement(&result, d("64100")).await;
        log.flush().await;

        let trades = std::fs::read_to_string(directory.path().join("trades.csv")).unwrap();
        assert!(trades.contains("ENTRY,7,btc-5m-7,UP,order-1"));
        assert!(trades.contains("WIN,7,btc-5m-7,UP"));
        let outcomes = std::fs::read_to_string(directory.path().join("outcomes.csv")).unwrap();
        assert!(outcomes.contains("7,btc-5m-7,WIN,UP,1.96"));
    }
}
