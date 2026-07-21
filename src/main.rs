//! Binance Prediction BTC five-minute trading bot.
//!
//! Flow: Binance BTCUSDT PriceSource → Value/Momentum Decider →
//! Binance Prediction MARKET/FOK Executor → Binance position settlement/redeem.

mod bot;
mod state;
mod tasks;

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use binance_5m_bot::config::Config;
use binance_5m_bot::tui;
use binance_5m_bot::tui::state::TuiState;
use tokio::sync::RwLock;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use bot::Bot;

fn load_dotenv() {
    if let Err(error) = dotenvy::dotenv() {
        if !error.not_found() {
            eprintln!("Warning: failed to load .env: {error}");
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    if let Err(error) = rustls::crypto::ring::default_provider().install_default() {
        anyhow::bail!("failed to install rustls crypto provider: {error:?}");
    }
    load_dotenv();

    let config_path = Path::new("config.toml");
    let config = if config_path.exists() {
        Config::load(config_path)?
    } else {
        let config = Config::default();
        config.save(config_path)?;
        config
    };
    config.validate()?;

    let log_dir = format!("logs/binance/{}", config.trading.mode);
    tokio::fs::create_dir_all(&log_dir).await?;
    let file_appender = tracing_appender::rolling::daily(&log_dir, "bot.log");
    let (file_writer, _guard) = tracing_appender::non_blocking(file_appender);
    let file_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    let console_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into());
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(file_writer)
                .with_ansi(false)
                .with_filter(file_filter),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(false)
                .with_filter(console_filter),
        )
        .init();
    tracing::info!("binance-5m-bot v{}", env!("CARGO_PKG_VERSION"));

    let tui_state = Arc::new(RwLock::new(TuiState {
        mode: config.trading.mode.to_string(),
        recent_trades: TuiState::load_trades_from_csv(&log_dir),
        ..TuiState::default()
    }));
    let mut bot = Bot::new(config, log_dir, Arc::clone(&tui_state)).await?;
    let shutdown = bot.shutdown_handle();
    let headless = std::env::var_os("BINANCE_5M_HEADLESS").is_some();
    let tui_handle = if headless {
        None
    } else {
        let tui_shutdown = Arc::clone(&shutdown);
        Some(std::thread::spawn(move || {
            if let Err(error) = tui::run(tui_state, tui_shutdown) {
                eprintln!("TUI error: {error}");
            }
        }))
    };

    let result = bot.run().await;
    shutdown.store(true, std::sync::atomic::Ordering::Release);
    if let Some(handle) = tui_handle {
        let _ = handle.join();
    }
    result
}
