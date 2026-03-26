//! polybot-tools — CLI utilities for Polymarket 5m Bot
//!
//! Commands:
//!   --derive-keys   Derive Polymarket CLOB API credentials
//!   --redeem-all    Redeem all winning positions from the last 24h
//!   --redeem <slug> Redeem a single market by slug

use anyhow::Result;
use polymarket_5m_bot::config::{Config, TradingMode};
use polymarket_5m_bot::data;
use secrecy::ExposeSecret;
use std::path::Path;

const WINDOWS_PER_DAY: i64 = 288;
const WINDOW_SECS: i64 = 300;

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

    if std::env::args().any(|a| a == "--derive-keys") {
        return derive_api_keys().await;
    }
    if std::env::args().any(|a| a == "--redeem-all") {
        return redeem_all().await;
    }
    if let Some(slug) = std::env::args().skip_while(|a| a != "--redeem").nth(1) {
        return redeem_one(&slug).await;
    }

    eprintln!("polybot-tools — CLI utilities for Polymarket 5m Bot");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  polybot-tools --derive-keys        Derive CLOB API credentials");
    eprintln!("  polybot-tools --redeem-all          Redeem all winning positions (last 24h)");
    eprintln!("  polybot-tools --redeem <slug>       Redeem a single market by slug");
    std::process::exit(1);
}

async fn redeem_all() -> Result<()> {
    use data::market_discovery;
    use data::polymarket;

    eprintln!("Scanning recent markets for redeemable positions...\n");

    let config_path = Path::new("config.json");
    let config = if config_path.exists() {
        Config::load(config_path)?
    } else {
        anyhow::bail!("config.json not found");
    };

    let private_key = if !config.trading.private_key.expose_secret().is_empty() {
        config.trading.private_key.expose_secret().to_owned()
    } else {
        anyhow::bail!("PRIVATE_KEY not set in .env");
    };

    let mode = if config.trading.mode.is_paper() {
        TradingMode::Live
    } else {
        config.trading.mode
    };
    let rpc = polymarket::rpc_url(mode);
    let redeemer = polymarket::CtfRedeemer::new(private_key, rpc);

    let gamma_url = &config.polyclob.gamma_api_url;
    let http = reqwest::Client::new();

    let now_ts = chrono::Utc::now().timestamp();
    let base_ts = (now_ts / WINDOW_SECS) * WINDOW_SECS;
    let mut condition_ids: Vec<(String, String)> = Vec::new();

    for i in 0..WINDOWS_PER_DAY {
        let ts = base_ts - i * WINDOW_SECS;
        let slug = format!("{}-{}", market_discovery::SERIES_ID, ts);
        let url = format!("{}/events?slug={}&limit=1", gamma_url, slug);

        let resp = match http.get(&url).send().await {
            Ok(r) if r.status().is_success() => r,
            _ => continue,
        };

        let data: serde_json::Value = match resp.json().await {
            Ok(d) => d,
            _ => continue,
        };

        if let Some(events) = data.as_array() {
            if let Some(event) = events.first() {
                if let Some(markets) = event.get("markets").and_then(|m| m.as_array()) {
                    for market in markets {
                        if let Some(cid) = market.get("conditionId").and_then(|c| c.as_str()) {
                            if !cid.is_empty() && !condition_ids.iter().any(|(id, _)| id == cid) {
                                condition_ids.push((cid.to_string(), slug.clone()));
                            }
                        }
                    }
                }
            }
        }
    }

    eprintln!(
        "Found {} markets with condition IDs. Checking positions...",
        condition_ids.len()
    );

    let redeemable = redeemer.find_redeemable(&condition_ids, 5).await?;

    if redeemable.is_empty() {
        eprintln!("No redeemable positions found.");
        return Ok(());
    }

    eprintln!(
        "{} redeemable positions found. Redeeming...\n",
        redeemable.len()
    );

    let mut success = 0u32;
    let mut failed = 0u32;

    for (cid, slug) in &redeemable {
        eprint!("  {} ({})... ", &cid[..10.min(cid.len())], slug);
        match redeemer.redeem(cid).await {
            Ok(tx) => {
                eprintln!("OK tx={}", tx);
                success += 1;
            }
            Err(e) => {
                eprintln!("FAIL: {}", e);
                failed += 1;
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }

    eprintln!("\nDone: {} redeemed, {} failed", success, failed);
    Ok(())
}

async fn redeem_one(slug: &str) -> Result<()> {
    use data::market_discovery::{self, GammaMarket, ResolutionState};
    use data::polymarket;

    let config_path = Path::new("config.json");
    let config = Config::load(config_path)?;

    let private_key = if !config.trading.private_key.expose_secret().is_empty() {
        config.trading.private_key.expose_secret().to_owned()
    } else {
        anyhow::bail!("PRIVATE_KEY not set in .env");
    };

    let mode = if config.trading.mode.is_paper() {
        TradingMode::Live
    } else {
        config.trading.mode
    };
    let rpc = polymarket::rpc_url(mode);
    let redeemer = polymarket::CtfRedeemer::new(private_key, rpc);

    let gamma_url = &config.polyclob.gamma_api_url;
    let http = reqwest::Client::new();
    let url = format!("{}/events?slug={}&limit=1", gamma_url, slug);

    let resp = http.get(&url).send().await?.error_for_status()?;
    let data: serde_json::Value = resp.json().await?;

    let cid = data
        .as_array()
        .and_then(|events| events.first())
        .and_then(|event| event.get("markets"))
        .and_then(|m| m.as_array())
        .and_then(|markets| markets.first())
        .and_then(|market| market.get("conditionId"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| anyhow::anyhow!("No conditionId found for slug: {}", slug))?
        .to_string();

    let market_json = data
        .as_array()
        .and_then(|events| events.first())
        .and_then(|event| event.get("markets"))
        .and_then(|m| m.as_array())
        .and_then(|markets| markets.first());

    let resolution = market_json.and_then(|m| {
        let state = market_discovery::infer_resolution_state(
            &serde_json::from_value::<GammaMarket>(m.clone()).ok()?,
        )?;
        Some(state)
    });

    eprintln!("Slug: {}", slug);
    eprintln!("Condition ID: {}", cid);
    match &resolution {
        Some(ResolutionState::Resolved(winner)) => eprintln!("Result: {} won", winner.as_str()),
        Some(ResolutionState::Pending) => eprintln!("Result: pending"),
        None => eprintln!("Result: unknown"),
    }

    match redeemer.has_redeemable_position(&cid).await {
        Ok(true) => {
            eprint!("Redeeming... ");
            match redeemer.redeem(&cid).await {
                Ok(tx) => eprintln!("OK tx={}", tx),
                Err(e) => eprintln!("FAIL: {}", e),
            }
        }
        Ok(false) => eprintln!("No winning position to redeem."),
        Err(e) => eprintln!("Check failed: {}", e),
    }

    Ok(())
}

async fn derive_api_keys() -> Result<()> {
    use polymarket_client_sdk::auth::{LocalSigner, Signer as _};
    use polymarket_client_sdk::clob;
    use polymarket_client_sdk::POLYGON;
    use secrecy::SecretString;
    use std::str::FromStr;

    eprintln!("Deriving Polymarket CLOB API credentials...");

    let private_key = SecretString::new(
        std::env::var("PRIVATE_KEY")
            .map_err(|_| anyhow::anyhow!("PRIVATE_KEY not set in .env"))?
            .into(),
    );
    let key_hex = private_key
        .expose_secret()
        .strip_prefix("0x")
        .unwrap_or(private_key.expose_secret());

    let signer = LocalSigner::from_str(key_hex)
        .map_err(|_| anyhow::anyhow!("Invalid PRIVATE_KEY in .env"))?
        .with_chain_id(Some(POLYGON));

    let client = clob::Client::default();
    let creds = client
        .create_or_derive_api_key(&signer, None)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to derive API key: {}", e))?;

    let api_key = creds.key().to_string();
    let secret = creds.secret().expose_secret().to_string();
    let passphrase = creds.passphrase().expose_secret().to_string();

    fn mask_secret(value: &str) -> String {
        if value.len() <= 8 {
            return "[redacted]".to_string();
        }
        format!("{}...{}", &value[..4], &value[value.len() - 4..])
    }

    eprintln!("Derived API credentials (not persisted to disk):");
    eprintln!("   POLY_API_KEY={}", api_key);
    eprintln!("   POLY_API_SECRET={}", mask_secret(&secret));
    eprintln!("   POLY_PASSPHRASE={}", mask_secret(&passphrase));
    eprintln!("\nThese credentials are derived on-the-fly during auth.");
    eprintln!("No secrets written to .env. Full secret values are intentionally masked.");

    Ok(())
}
