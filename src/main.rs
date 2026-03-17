//! Polymarket 5m Bot — Pipeline Architecture
//!
//! Flow: PriceSource → SignalComputer → TradeDecider → OrderExecutor → Settler

mod config;
mod data;
mod pipeline;
mod signing;

use anyhow::Result;
use chrono::Utc;
use config::Config;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing_subscriber::fmt::writer::MakeWriterExt;

const LOG_DIR: &str = "logs";

use data::coinbase::CoinbaseClient;
use data::market_discovery::{DiscoveryConfig, MarketDiscovery};
use data::polymarket::PolymarketClient;

use pipeline::price_source::PriceSource;
use pipeline::signal;
use pipeline::signal::Direction;
use pipeline::decider::{self, DeciderConfig, AccountState};
use pipeline::executor::Executor;
use pipeline::settler::{Settler, PendingPosition};

// ─── Bot State ───

struct BotState {
    /// Counter for throttling NO TRADE logs
    no_trade_count: u64,
    /// Last logged reason (to avoid spamming same reason)
    last_no_trade_reason: String,
}

impl BotState {
    fn new() -> Self {
        Self {
            no_trade_count: 0,
            last_no_trade_reason: String::new(),
        }
    }
}

// ─── Bot ───

struct Bot {
    config: Config,
    price_source: Arc<PriceSource>,
    polymarket: Arc<PolymarketClient>,
    discovery: Arc<MarketDiscovery>,
    state: Arc<RwLock<BotState>>,
    account: Arc<RwLock<AccountState>>,
    settler: Arc<RwLock<Settler>>,
    executor: Executor,
    // Dynamic market data
    active_token_yes: Arc<RwLock<String>>,
    active_token_no: Arc<RwLock<String>>,
    active_settlement_ms: Arc<RwLock<i64>>,
}

impl Bot {
    async fn new(config: Config) -> Result<Self> {
        let coinbase = Arc::new(CoinbaseClient::new("BTC-USD"));
        let price_source = Arc::new(PriceSource::new(coinbase, 1000));
        let polymarket = Arc::new(PolymarketClient::new());

        let resolved_series_id = config.market.resolve_series_id();
        if resolved_series_id.is_empty() {
            anyhow::bail!("series_id is empty. Set market.event_url in config.json");
        }

        let discovery_cfg = DiscoveryConfig {
            series_id: resolved_series_id,
            gamma_api_url: config.polyclob.gamma_api_url.clone(),
            refresh_interval_sec: 60,
            window_minutes: config.market.window_minutes,
        };
        let discovery = Arc::new(MarketDiscovery::new(discovery_cfg));

        let executor = Executor::new(
            config.trading.mode.clone(),
            config.trading.private_key.clone(),
            PolymarketClient::new(),
        );

        // Load balance from file or use default
        let initial_balance = Self::load_balance().unwrap_or(1000.0);
        tracing::info!("[INIT] Starting balance: ${:.2}", initial_balance);

        Ok(Self {
            config,
            price_source,
            polymarket,
            discovery,
            state: Arc::new(RwLock::new(BotState::new())),
            account: Arc::new(RwLock::new(AccountState::new(initial_balance))),
            settler: Arc::new(RwLock::new(Settler::new())),
            executor,
            active_token_yes: Arc::new(RwLock::new(String::new())),
            active_token_no: Arc::new(RwLock::new(String::new())),
            active_settlement_ms: Arc::new(RwLock::new(0)),
        })
    }

    fn load_balance() -> Option<f64> {
        let content = std::fs::read_to_string(Path::new(LOG_DIR).join("balance")).ok()?;
        content.trim().parse().ok()
    }

    async fn run(&mut self) -> Result<()> {
        tracing::info!("[INIT] mode={} interval={}ms", self.config.trading.mode, self.config.polling.signal_interval_ms);

        self.refresh_market().await;
        self.price_source.clone().start().await;

        let mut settlement_handle = self.start_settlement_checker();
        let mut refresher_handle = self.start_market_refresher();

        let mut tick = interval(Duration::from_millis(self.config.polling.signal_interval_ms));

        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if let Err(e) = self.tick().await {
                        tracing::error!("[BOT] Tick error: {}", e);
                    }
                }
                result = &mut settlement_handle => {
                    match result {
                        Ok(()) => tracing::error!("[BOT] Settlement checker exited unexpectedly"),
                        Err(e) => tracing::error!("[BOT] Settlement checker panicked: {}", e),
                    }
                    break;
                }
                result = &mut refresher_handle => {
                    match result {
                        Ok(()) => tracing::error!("[BOT] Market refresher exited unexpectedly"),
                        Err(e) => tracing::error!("[BOT] Market refresher panicked: {}", e),
                    }
                    break;
                }
            }
        }

        Ok(())
    }

    fn start_market_refresher(&self) -> tokio::task::JoinHandle<()> {
        let discovery = self.discovery.clone();
        let token_yes = self.active_token_yes.clone();
        let token_no = self.active_token_no.clone();
        let settle_ms = self.active_settlement_ms.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                match discovery.discover().await {
                    Ok(active) => {
                        let current_yes = token_yes.read().await.clone();
                        if current_yes != active.token_id_yes {
                            tracing::info!("[MKT] {} ends {}", active.market.slug, active.end_date);
                            *token_yes.write().await = active.token_id_yes;
                            *token_no.write().await = active.token_id_no;
                            *settle_ms.write().await = active.end_date.timestamp_millis();
                        }
                    }
                    Err(e) => {
                        tracing::debug!("[MARKET] Market refresh failed: {}", e);
                    }
                }
            }
        })
    }

    async fn tick(&self) -> Result<()> {
        // 1. Get latest price
        let prices = self.price_source.history().await;
        let closes: Vec<f64> = prices.iter().map(|p| p.price).collect();
        let btc_price = match self.price_source.latest().await {
            Some(p) => p,
            None => return Ok(()), // No price data yet
        };

        if closes.len() < 60 {
            return Ok(()); // Need more data
        }

        // 2. Get market data FIRST (signal depends on it)
        let (poly_yes, poly_no, settlement_ms) = {
            let token_yes = self.active_token_yes.read().await.clone();
            let token_no = self.active_token_no.read().await.clone();
            let settle = *self.active_settlement_ms.read().await;

            if token_yes.is_empty() || token_no.is_empty() {
                // Log buffer status for debugging
                if closes.len() % 30 == 0 {
                    tracing::debug!("[DEBUG] Waiting for market tokens | buffer={}", closes.len());
                }
                return Ok(());
            }

            let yes = self.polymarket.fetch_mid_price(&token_yes).await;
            let no = self.polymarket.fetch_mid_price(&token_no).await;
            
            match (&yes, &no) {
                (Ok(y), Ok(n)) => {
                    tracing::debug!("[PRICE] Yes={:.3} No={:.3} | buffer={}", y, n, closes.len());
                }
                (Err(e), _) | (_, Err(e)) => {
                    tracing::warn!("[PRICE] Polymarket fetch failed: {}", e);
                }
            }
            
            (yes.ok(), no.ok(), settle)
        };

        // 3. Compute signal based on market prices (Stage 2)
        if !signal::is_market_extreme(poly_yes, poly_no, self.config.strategy.extreme_threshold) {
            return Ok(());
        }

        // 4. Decide trade (Stage 3)
        let account_read = self.account.read().await.clone();

        let decider_cfg = DeciderConfig {
            edge_threshold: self.config.edge.edge_threshold_early,
            max_position: self.config.strategy.max_position_size,
            min_position: self.config.strategy.min_order_size,
            cooldown_ms: 5_000,
            max_risk_fraction: 0.10,
            extreme_threshold: self.config.strategy.extreme_threshold,
            fair_value: self.config.strategy.fair_value,
            max_consecutive_losses: self.config.risk.max_consecutive_losses,
            max_daily_loss_pct: self.config.risk.max_daily_loss_pct,
        };

        let decision = decider::decide(
            poly_yes,
            poly_no,
            settlement_ms,
            &account_read,
            &decider_cfg,
            &closes,
        );

        // 6. Execute trade (Stage 4)
        match &decision {
            decider::Decision::Pass(reason) => {
                {
                    let mut st = self.state.write().await;
                    st.no_trade_count += 1;
                    // Compare category only (strip trailing numbers/%)
                    let category = reason.trim_end_matches(|c: char| c.is_ascii_digit() || c == '%' || c == '_');
                    let prev_cat = st.last_no_trade_reason.trim_end_matches(|c: char| c.is_ascii_digit() || c == '%' || c == '_');
                    let changed = category != prev_cat;
                    if changed { st.last_no_trade_reason = reason.clone(); }
                    if changed && !reason.contains("cooldown") && !reason.contains("loss_pause") {
                        tracing::info!("[SKIP] {} | BTC=${:.0}", reason, btc_price);
                    }
                }
            }
            decider::Decision::Trade { direction, size_usdc: _, edge } => {
                let token_yes = self.active_token_yes.read().await.clone();
                let token_no = self.active_token_no.read().await.clone();

                let cheap_price = match direction {
                    Direction::Up => poly_yes.unwrap_or(0.5),
                    Direction::Down => poly_no.unwrap_or(0.5),
                };

                tracing::info!(
                    "[TRADE] {} @ {:.3} edge={:.0}% BTC=${:.0}",
                    direction.as_str(), cheap_price, edge * 100.0, btc_price,
                );

                if let Some(order) = self.executor.execute(
                    &decision,
                    &token_yes,
                    &token_no,
                    poly_yes,
                    poly_no,
                    settlement_ms,
                    btc_price,
                ).await {
                    // Update account
                    {
                        let mut acc = self.account.write().await;
                        acc.record_trade(order.cost);
                        acc.record_trade_for_market(settlement_ms);
                    }

                    // Add to settler
                    self.settler.write().await.add_position(PendingPosition {
                        direction: order.direction,
                        size_usdc: order.size_usdc,
                        entry_price: order.entry_price,
                        cost: order.cost,
                        settlement_time_ms: order.settlement_time_ms,
                        entry_btc_price: order.entry_btc_price,
                    });

                    // Log to file
                    let bal = self.account.read().await.balance;
                    if let Ok(mut file) = std::fs::OpenOptions::new()
                        .create(true).append(true).open(Path::new(LOG_DIR).join("trades.csv"))
                    {
                        let _ = writeln!(file, "{},{},{},{:.3},{:.2},{:.1},{:.2}",
                            Utc::now().format("%H:%M:%S"),
                            order.direction.as_str(),
                            &order.order_id[..8],
                            order.entry_price,
                            order.cost,
                            edge * 100.0,
                            bal,
                        );
                    }
                }
            }
        }

        Ok(())
    }

    async fn refresh_market(&self) {
        match self.discovery.discover().await {
            Ok(active) => {
                tracing::info!("[MKT] {} ends {}", active.market.slug, active.end_date);
                *self.active_token_yes.write().await = active.token_id_yes.clone();
                *self.active_token_no.write().await = active.token_id_no.clone();
                *self.active_settlement_ms.write().await = active.end_date.timestamp_millis();
            }
            Err(e) => {
                tracing::warn!("[MKT] discovery failed: {}", e);
            }
        }
    }

    fn start_settlement_checker(&self) -> tokio::task::JoinHandle<()> {
        let settler = self.settler.clone();
        let account = self.account.clone();
        let price_source = self.price_source.clone();
        let btc_tiebreaker_usd = self.config.strategy.btc_tiebreaker_usd;
        let rpc = data::chainlink::rpc_url(&self.config.trading.mode);

        tokio::spawn(async move {
            let http = reqwest::Client::new();
            let mut interval = tokio::time::interval(Duration::from_secs(15));
            loop {
                interval.tick().await;

                let btc_price = match data::chainlink::fetch_btc_price(&http, &rpc).await {
                    Ok(p) => p,
                    Err(e) => {
                        // Fallback to Coinbase if Chainlink fails
                        tracing::debug!("[SETTLE] Chainlink failed: {}, using Coinbase", e);
                        match price_source.latest().await {
                            Some(p) => p,
                            None => continue,
                        }
                    }
                };

                let results = settler.write().await.check_settlements(btc_price, btc_tiebreaker_usd);
                if !results.is_empty() {
                    let mut acc = account.write().await;
                    for r in &results {
                        acc.record_settlement(r);
                    }

                    tracing::info!(
                        "[BAL] ${:.2} pnl=${:+.2} settled={}",
                        acc.balance, acc.daily_pnl, results.len(),
                    );

                    let bal = acc.balance;
                    drop(acc);
                    let _ = std::fs::write(
                        Path::new(LOG_DIR).join("balance"),
                        format!("{:.2}", bal),
                    );
                }
            }
        })
    }
}

// ─── Main ───

fn load_dotenv() {
    if let Ok(content) = std::fs::read_to_string(".env") {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue; }
            if let Some((k, v)) = line.split_once('=') {
                std::env::set_var(k.trim(), v.trim());
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    load_dotenv();
    std::fs::create_dir_all(LOG_DIR).ok();

    let file_appender = tracing_appender::rolling::never(LOG_DIR, "bot.log");
    let (file_writer, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with_writer(file_writer.and(std::io::stderr))
        .with_ansi(false)
        .init();
    tracing::info!("polybot v0.3.0");

    let config_path = Path::new("config.json");
    let config = if config_path.exists() {
        Config::load(config_path).unwrap_or_else(|e| {
            tracing::warn!("[INIT] Failed to load config: {}, using defaults", e);
            Config::default()
        })
    } else {
        let cfg = Config::default();
        let _ = cfg.save(config_path);
        cfg
    };

    let mut bot = Bot::new(config).await?;
    bot.run().await?;

    Ok(())
}
