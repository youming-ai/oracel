//! Binance Prediction diagnostics and manual recovery tools.

use std::path::Path;

use anyhow::Result;
use binance_5m_bot::config::Config;
use binance_5m_bot::data::binance_prediction::BinancePredictionClient;

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
    let config = Config::load(Path::new("config.toml"))?;
    config.validate()?;

    if std::env::args().any(|argument| argument == "--check") {
        return check(&config).await;
    }
    if let Some(token_id) = std::env::args()
        .skip_while(|argument| argument != "--redeem")
        .nth(1)
    {
        return redeem(&config, &token_id).await;
    }

    eprintln!("binance-5m-tools — Binance Prediction diagnostics");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  binance-5m-tools --check");
    eprintln!("  binance-5m-tools --redeem <prediction-token-id>");
    std::process::exit(1);
}

async fn check(config: &Config) -> Result<()> {
    let client = BinancePredictionClient::connect(&config.binance_prediction, false).await?;
    let wallets = client.list_wallets().await?;
    let balance = client.payment_balance().await?;
    let market = client
        .discover_active_market(chrono::Utc::now().timestamp_millis())
        .await?;
    println!("Binance Prediction API: OK");
    println!(
        "Payment account: {}",
        config.binance_prediction.payment_account.as_api_str()
    );
    println!("Available balance: {balance:.8} USDT");
    println!("Registered wallets: {}", wallets.len());
    for wallet in wallets {
        println!("  {} {}", wallet.wallet_id, wallet.wallet_address);
    }
    println!("Active BTC market: {}", market.slug);
    println!(
        "  topic={} start={} end={}",
        market.market_topic_id, market.start_ms, market.end_ms
    );
    println!(
        "  reference={} UP={} DOWN={}",
        market.reference_price, market.up.token_id, market.down.token_id
    );
    Ok(())
}

async fn redeem(config: &Config, token_id: &str) -> Result<()> {
    let client = BinancePredictionClient::connect(&config.binance_prediction, true).await?;
    let receipt = client.redeem(token_id).await?;
    println!("Redeem submitted");
    println!("status={}", receipt.status);
    if let Some(batch_id) = receipt.batch_id {
        println!("batch_id={batch_id}");
    }
    if let Some(transaction_hash) = receipt.transaction_hash {
        println!("tx_hash={transaction_hash}");
    }
    Ok(())
}
