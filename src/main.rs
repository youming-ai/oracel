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

    let config_path = Path::new("config.toml");
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

    // Write time windows config for dashboard consumption (atomic write)
    {
        let tw_tmp = std::path::Path::new(&log_dir).join("time_windows.json.tmp");
        let tw_dst = std::path::Path::new(&log_dir).join("time_windows.json");
        let tw_json = serde_json::json!({
            "window1": {
                "start": config.time_windows.window1_start,
                "end": config.time_windows.window1_end,
                "label": format!("{:02}:00-{:02}:00 UTC", config.time_windows.window1_start, config.time_windows.window1_end)
            },
            "window2": {
                "start": config.time_windows.window2_start,
                "end": config.time_windows.window2_end,
                "label": format!("{:02}:00-{:02}:00 UTC", config.time_windows.window2_start, config.time_windows.window2_end)
            }
        });
        let content = serde_json::to_string_pretty(&tw_json).unwrap();
        if let Err(e) = tokio::fs::write(&tw_tmp, &content).await {
            eprintln!("[INIT] Failed to write time_windows.json: {}", e);
        } else if let Err(e) = tokio::fs::rename(&tw_tmp, &tw_dst).await {
            eprintln!("[INIT] Failed to rename time_windows.json: {}", e);
        }
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
            "[INIT] Running live mode with default config values; review config.toml before trading"
        );
    }

    if config.trading.mode.is_live() && config.trading.private_key.expose_secret().is_empty() {
        anyhow::bail!("PRIVATE_KEY not set in .env — required for live trading");
    }

    let mut bot = Bot::new(config, log_dir).await?;
    bot.run().await?;

    Ok(())
}
