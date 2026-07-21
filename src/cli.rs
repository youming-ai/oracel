//! Binance Prediction account and market diagnostics.

use anyhow::Result;
use polymarket_5m_bot::data::binance_prediction::BinancePredictionClient;

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

    if std::env::args().any(|argument| argument == "--binance-prediction-check") {
        let client = BinancePredictionClient::from_env()?;
        let report = client.access_report().await?;
        println!("{}", serde_json::to_string_pretty(&report)?);
        if report.wallets.is_empty() {
            anyhow::bail!("no Binance Prediction wallet is registered for this API account");
        }
        if report.btc_5m_candidates.is_empty() {
            anyhow::bail!("no BTC five-minute Prediction market is visible to this API account");
        }
        return Ok(());
    }

    eprintln!("polybot-tools — Binance Prediction diagnostics");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  polybot-tools --binance-prediction-check");
    std::process::exit(1);
}
