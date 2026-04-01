//! Polymarket 5m Bot — Pipeline Architecture
//!
//! Flow: PriceSource → SignalComputer → TradeDecider → OrderExecutor → Settler

mod bot;
mod state;
mod tasks;

use anyhow::Result;
use bot::Bot;
use polymarket_5m_bot::config::Config;
use secrecy::ExposeSecret;
use std::path::Path;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

fn load_dotenv() {
    if let Err(e) = dotenvy::dotenv() {
        if !e.not_found() {
            eprintln!("Warning: failed to load .env: {}", e);
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    if let Err(e) = rustls::crypto::ring::default_provider().install_default() {
        eprintln!("Failed to install rustls crypto provider: {:?}", e);
        std::process::exit(1);
    }

    load_dotenv();

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

    let log_dir = format!("logs/{}", config.trading.mode);
    if let Err(e) = tokio::fs::create_dir_all(&log_dir).await {
        eprintln!("[INIT] Failed to create log dir {}: {}", log_dir, e);
        std::process::exit(1);
    }

    let file_appender = tracing_appender::rolling::daily(&log_dir, "bot.log");
    let (file_writer, _guard) = tracing_appender::non_blocking(file_appender);

    let file_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    let console_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into());

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file_writer)
        .with_ansi(false)
        .with_filter(file_filter);

    let console_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_filter(console_filter);

    tracing_subscriber::registry()
        .with(file_layer)
        .with(console_layer)
        .init();
    tracing::info!("polybot v{}", env!("CARGO_PKG_VERSION"));

    if config.trading.mode.is_live() && config.is_default_non_trading() {
        tracing::warn!(
            "[INIT] Running live mode with default config values; review config.json before trading"
        );
    }

    if config.trading.mode.is_live() && config.trading.private_key.expose_secret().is_empty() {
        anyhow::bail!("PRIVATE_KEY not set in .env — required for live trading");
    }

    let mut bot = Bot::new(config, log_dir).await?;
    bot.run().await?;

    Ok(())
}
