//! Polymarket 5m Bot — Pipeline Architecture
//!
//! Flow: PriceSource → SignalComputer → TradeDecider → OrderExecutor → Settler

mod config;
mod data;
mod pipeline;

use anyhow::Result;
use chrono::Utc;
use config::{Config, TradingMode};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use secrecy::{ExposeSecret, SecretString};
use std::path::Path;
use std::sync::Arc;
#[cfg(unix)]
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing_subscriber::fmt::writer::MakeWriterExt;

const PRICE_BUFFER_MAX: usize = 1000;
const WINDOWS_PER_DAY: i64 = 288;
const WINDOW_SECS: i64 = 300;

fn decimal(value: &str) -> Decimal {
    Decimal::from_str_exact(value).expect("valid decimal literal")
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct PersistState {
    pending_positions: Vec<PendingPosition>,
    last_traded_settlement_ms: i64,
    #[serde(default)]
    consecutive_losses: u32,
    #[serde(default)]
    consecutive_wins: u32,
    #[serde(default)]
    total_wins: u32,
    #[serde(default)]
    total_losses: u32,
    #[serde(default)]
    pause_until_ms: i64,
    #[serde(default)]
    daily_pnl: String,
    #[serde(default)]
    pnl_reset_date: String,
}

use data::coinbase::CoinbaseClient;
use data::market_discovery::{
    infer_resolution_state, DiscoveryConfig, MarketDiscovery, ResolutionState,
};
use data::polymarket::{AuthenticatedPolyClient, CtfRedeemer, PolymarketClient};

use pipeline::decider::{self, AccountState, DeciderConfig};
use pipeline::executor::{ExecuteContext, Executor};
use pipeline::price_source::PriceSource;
use pipeline::settler::{PendingPosition, Settler};
use pipeline::signal;
use pipeline::signal::Direction;

// ─── Bot State ───

struct BotState {
    /// Counter for throttling NO TRADE logs
    no_trade_count: u64,
    /// Last logged reason (to avoid spamming same reason)
    last_no_trade_reason: String,
    /// Last idle reason from pre-signal filters (log on state change only)
    last_idle_reason: String,
}

impl BotState {
    fn new() -> Self {
        Self {
            no_trade_count: 0,
            last_no_trade_reason: String::new(),
            last_idle_reason: String::new(),
        }
    }

    fn log_idle_change(&mut self, reason: &str, detail: &str) {
        if self.last_idle_reason != reason {
            self.last_idle_reason = reason.to_string();
            tracing::info!("[IDLE] {} | {}", reason, detail);
        }
    }

    fn clear_idle(&mut self) {
        self.last_idle_reason.clear();
    }
}

#[derive(Debug, Clone, Default)]
struct MarketState {
    token_yes: String,
    token_no: String,
    condition_id: String,
    market_slug: String,
    settlement_ms: i64,
}

impl MarketState {
    fn is_ready(&self) -> bool {
        !self.token_yes.is_empty() && !self.token_no.is_empty()
    }
}

// ─── Bot ───

struct Bot {
    config: Config,
    log_dir: String,
    price_source: Arc<PriceSource>,
    polymarket: Arc<PolymarketClient>,
    discovery: Arc<MarketDiscovery>,
    state: Arc<RwLock<BotState>>,
    account: Arc<RwLock<AccountState>>,
    settler: Arc<RwLock<Settler>>,
    executor: Executor,
    redeemer: Option<Arc<CtfRedeemer>>,
    // Dynamic market data
    market_state: Arc<RwLock<MarketState>>,
}

impl Bot {
    async fn new(config: Config, log_dir: String) -> Result<Self> {
        let coinbase = Arc::new(CoinbaseClient::new("BTC-USD"));
        let price_source = Arc::new(PriceSource::new(coinbase, PRICE_BUFFER_MAX));
        let polymarket = Arc::new(PolymarketClient::new()?);

        let discovery_cfg = DiscoveryConfig {
            gamma_api_url: config.polyclob.gamma_api_url.clone(),
        };
        let discovery = Arc::new(MarketDiscovery::new(discovery_cfg));

        let auth_client = if config.trading.mode.is_live()
            && !config.trading.private_key.expose_secret().is_empty()
        {
            match AuthenticatedPolyClient::new(config.trading.private_key.expose_secret()).await {
                Ok(c) => {
                    tracing::info!("[INIT] Authenticated with Polymarket CLOB");
                    Some(c)
                }
                Err(e) => {
                    anyhow::bail!("[INIT] CLOB auth failed: {} — cannot run in live mode", e);
                }
            }
        } else {
            None
        };

        let executor = Executor::new(config.trading.mode, auth_client);

        // Create CTF redeemer for live mode
        let redeemer = if config.trading.mode.is_live()
            && !config.trading.private_key.expose_secret().is_empty()
        {
            let rpc = data::chainlink::rpc_url(config.trading.mode);
            tracing::info!("[INIT] CTF redeemer enabled for on-chain redemption");
            Some(Arc::new(CtfRedeemer::new(
                config.trading.private_key.expose_secret().to_owned(),
                rpc,
            )))
        } else {
            None
        };

        let initial_balance = if config.trading.mode.is_paper() {
            Self::load_balance(&log_dir)
                .await
                .unwrap_or_else(|| decimal("1000"))
        } else {
            Self::load_balance(&log_dir).await.unwrap_or(Decimal::ZERO)
        };
        tracing::info!("[INIT] Starting balance: ${:.2}", initial_balance);
        Self::write_balance(&log_dir, initial_balance).await;

        let mut settler = Settler::new();
        let mut account = AccountState::new(initial_balance);

        let saved = Self::load_state(&log_dir).await;
        if !saved.pending_positions.is_empty() {
            tracing::info!(
                "[INIT] Restored {} pending position(s) from state.json",
                saved.pending_positions.len()
            );
            settler.restore_positions(saved.pending_positions);
        }
        if saved.last_traded_settlement_ms > 0 {
            account.record_trade_for_market(saved.last_traded_settlement_ms);
        }
        account.consecutive_losses = saved.consecutive_losses;
        account.consecutive_wins = saved.consecutive_wins;
        account.total_wins = saved.total_wins;
        account.total_losses = saved.total_losses;
        account.pause_until_ms = saved.pause_until_ms;
        if let Ok(pnl) = Decimal::from_str_exact(&saved.daily_pnl) {
            account.daily_pnl = pnl;
        }
        if !saved.pnl_reset_date.is_empty() {
            account.pnl_reset_date = saved.pnl_reset_date;
        }
        account.check_daily_reset();

        Ok(Self {
            config,
            log_dir,
            price_source,
            polymarket,
            discovery,
            state: Arc::new(RwLock::new(BotState::new())),
            account: Arc::new(RwLock::new(account)),
            settler: Arc::new(RwLock::new(settler)),
            executor,
            redeemer,
            market_state: Arc::new(RwLock::new(MarketState::default())),
        })
    }

    async fn load_balance(log_dir: &str) -> Option<Decimal> {
        let content = tokio::fs::read_to_string(Path::new(log_dir).join("balance"))
            .await
            .ok()?;
        content.trim().parse().ok()
    }

    async fn write_balance(log_dir: &str, bal: Decimal) {
        let tmp = Path::new(log_dir).join("balance.tmp");
        let dst = Path::new(log_dir).join("balance");
        let text = format!("{:.2}", bal);
        let _ = tokio::fs::write(&tmp, &text).await;
        let _ = tokio::fs::rename(&tmp, &dst).await;
    }

    async fn load_state(log_dir: &str) -> PersistState {
        let path = Path::new(log_dir).join("state.json");
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => PersistState::default(),
        }
    }

    async fn save_state(
        log_dir: &str,
        settler: &Arc<RwLock<Settler>>,
        account: &Arc<RwLock<AccountState>>,
    ) {
        let positions = settler.read().await.pending_positions();
        let acc = account.read().await;
        let state = PersistState {
            pending_positions: positions,
            last_traded_settlement_ms: acc.last_traded_settlement_ms,
            consecutive_losses: acc.consecutive_losses,
            consecutive_wins: acc.consecutive_wins,
            total_wins: acc.total_wins,
            total_losses: acc.total_losses,
            pause_until_ms: acc.pause_until_ms,
            daily_pnl: acc.daily_pnl.to_string(),
            pnl_reset_date: acc.pnl_reset_date.clone(),
        };
        drop(acc);
        let tmp = Path::new(log_dir).join("state.json.tmp");
        let dst = Path::new(log_dir).join("state.json");
        if let Ok(json) = serde_json::to_string(&state) {
            let _ = tokio::fs::write(&tmp, &json).await;
            let _ = tokio::fs::rename(&tmp, &dst).await;
        }
    }

    async fn run(&mut self) -> Result<()> {
        tracing::info!(
            "[INIT] mode={} interval={}ms",
            self.config.trading.mode,
            self.config.polling.signal_interval_ms
        );

        self.refresh_market().await;
        self.price_source.clone().start().await;

        #[cfg(unix)]
        let mut sigint = signal(SignalKind::interrupt())?;
        #[cfg(unix)]
        let mut sigterm = signal(SignalKind::terminate())?;

        #[cfg(unix)]
        let shutdown_signal = async {
            tokio::select! {
                _ = sigint.recv() => "SIGINT",
                _ = sigterm.recv() => "SIGTERM",
            }
        };

        #[cfg(not(unix))]
        let shutdown_signal = async {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to listen for ctrl-c");
            "SIGINT"
        };

        tokio::pin!(shutdown_signal);

        let mut settlement_handle = self.start_settlement_checker();
        let mut refresher_handle = self.start_market_refresher();
        let mut status_handle = self.start_status_printer();

        let mut tick = interval(Duration::from_millis(
            self.config.polling.signal_interval_ms,
        ));

        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if let Err(e) = self.tick().await {
                        tracing::error!("[BOT] Tick error: {}", e);
                    }
                }
                signal = &mut shutdown_signal => {
                    tracing::info!("[BOT] Received {}, cleaning up...", signal);
                    break;
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
                result = &mut status_handle => {
                    match result {
                        Ok(()) => tracing::error!("[BOT] Status printer exited unexpectedly"),
                        Err(e) => tracing::error!("[BOT] Status printer panicked: {}", e),
                    }
                    break;
                }
            }
        }

        Ok(())
    }

    fn start_market_refresher(&self) -> tokio::task::JoinHandle<()> {
        let discovery = self.discovery.clone();
        let market_state = self.market_state.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                match discovery.discover().await {
                    Ok(active) => {
                        let current_yes = market_state.read().await.token_yes.clone();
                        if current_yes != active.token_id_yes {
                            tracing::info!("[MKT] {} ends {}", active.market.slug, active.end_date);
                            *market_state.write().await = MarketState {
                                token_yes: active.token_id_yes,
                                token_no: active.token_id_no,
                                condition_id: active.condition_id,
                                market_slug: active.market.slug,
                                settlement_ms: active.end_date.timestamp_millis(),
                            };
                        }
                    }
                    Err(e) => {
                        tracing::debug!("[MARKET] Market refresh failed: {}", e);
                    }
                }
            }
        })
    }

    fn start_status_printer(&self) -> tokio::task::JoinHandle<()> {
        let price_source = self.price_source.clone();
        let account = self.account.clone();
        let settler = self.settler.clone();
        let market_state = self.market_state.clone();
        let mode = self.config.trading.mode;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));
            loop {
                interval.tick().await;

                let btc = price_source.latest().await.unwrap_or(0.0);
                let acc = account.read().await;
                let pending = settler.read().await.pending_count();
                let settle = market_state.read().await.settlement_ms;

                let ttl = if settle > 0 {
                    let remaining_s = (settle - Utc::now().timestamp_millis()) / 1000;
                    if remaining_s > 0 {
                        format!("{}m{}s", remaining_s / 60, remaining_s % 60)
                    } else {
                        "expired".into()
                    }
                } else {
                    "?".into()
                };

                tracing::info!(
                    "[STATUS] {} | BTC=${:.0} | bal=${:.2} pnl={:+.2} | {}W/{}L streak={} | pending={} | ttl={}",
                    mode, btc, acc.balance, acc.daily_pnl,
                    acc.total_wins, acc.total_losses,
                    if acc.consecutive_wins > 0 { format!("+{}", acc.consecutive_wins) }
                    else if acc.consecutive_losses > 0 { format!("-{}", acc.consecutive_losses) }
                    else { "0".into() },
                    pending, ttl,
                );
            }
        })
    }

    async fn tick(&self) -> Result<()> {
        self.account.write().await.check_daily_reset();

        if self.config.trading.mode.is_live() {
            let rpc = data::chainlink::rpc_url(self.config.trading.mode);
            if let Some(ref redeemer) = self.redeemer {
                match redeemer.wallet_address() {
                    Ok(wallet) => match data::polymarket::query_usdc_balance(&rpc, wallet).await {
                        Ok(on_chain_bal) => {
                            self.account.write().await.balance = on_chain_bal;
                            Self::write_balance(&self.log_dir, on_chain_bal).await;
                        }
                        Err(e) => {
                            tracing::warn!("[BAL] Failed to query on-chain USDC balance: {}", e);
                        }
                    },
                    Err(e) => {
                        tracing::warn!("[BAL] Failed to derive wallet address: {}", e);
                    }
                }
            }
        }

        let mkt = self.market_state.read().await.clone();

        // 1. Get latest price
        let prices = self.price_source.history().await;
        let closes: Vec<f64> = prices.iter().map(|p| p.price).collect();
        let btc_price = match self.price_source.latest().await {
            Some(p) => p,
            None => return Ok(()),
        };

        if closes.len() < 60 {
            let detail = format!("buffer={}/60", closes.len());
            self.state
                .write()
                .await
                .log_idle_change("buffer_filling", &detail);
            return Ok(());
        }

        const STALE_THRESHOLD_MS: i64 = 30_000;
        if let Some(last_ts) = self.price_source.last_tick_ms().await {
            let age = Utc::now().timestamp_millis() - last_ts;
            if age > STALE_THRESHOLD_MS {
                tracing::warn!("[PRICE] BTC data stale ({}s), skipping trade", age / 1000);
                return Ok(());
            }
        }

        // 2. Get market data FIRST (signal depends on it)
        if !mkt.is_ready() {
            self.state
                .write()
                .await
                .log_idle_change("market_not_ready", "waiting for token IDs");
            return Ok(());
        }

        const MIN_TTL_MS: i64 = 30_000;
        if mkt.settlement_ms > 0 {
            let remaining = mkt.settlement_ms - Utc::now().timestamp_millis();
            if remaining < MIN_TTL_MS {
                let detail = format!("remaining={}s", remaining / 1000);
                self.state
                    .write()
                    .await
                    .log_idle_change("ttl_too_short", &detail);
                return Ok(());
            }
        }

        let yes = self.polymarket.fetch_mid_price(&mkt.token_yes).await;
        let no = self.polymarket.fetch_mid_price(&mkt.token_no).await;

        match (&yes, &no) {
            (Ok(y), Ok(n)) => {
                tracing::debug!("[PRICE] Yes={:.3} No={:.3} | buffer={}", y, n, closes.len());
            }
            (Err(e), _) | (_, Err(e)) => {
                tracing::warn!("[PRICE] Polymarket fetch failed: {}", e);
            }
        }

        let poly_yes = yes.ok();
        let poly_no = no.ok();
        let settlement_ms = mkt.settlement_ms;

        let poly_yes_dec = poly_yes.and_then(|v| Decimal::try_from(v).ok());
        let poly_no_dec = poly_no.and_then(|v| Decimal::try_from(v).ok());

        let yes_f64 = poly_yes;
        let no_f64 = poly_no;
        let extreme_f64 = self
            .config
            .strategy
            .extreme_threshold
            .to_f64()
            .unwrap_or(0.80);
        if !signal::is_market_extreme(yes_f64, no_f64, extreme_f64) {
            let detail = format!(
                "Yes={:.3} No={:.3} thr={:.2}",
                yes_f64.unwrap_or(0.0),
                no_f64.unwrap_or(0.0),
                extreme_f64,
            );
            self.state
                .write()
                .await
                .log_idle_change("not_extreme", &detail);
            return Ok(());
        }

        self.state.write().await.clear_idle();

        // 4. Decide trade (Stage 3)
        let account_read = self.account.read().await.clone();

        let decider_cfg = DeciderConfig {
            edge_threshold: self.config.edge.edge_threshold_early,
            cooldown_ms: self.config.risk.cooldown_ms,
            extreme_threshold: self.config.strategy.extreme_threshold,
            fair_value: self.config.strategy.fair_value,
            max_consecutive_losses: self.config.risk.max_consecutive_losses,
            max_daily_loss_pct: self.config.risk.max_daily_loss_pct,
            momentum_threshold: self.config.strategy.momentum_threshold,
            momentum_lookback_ms: self.config.strategy.momentum_lookback_ms,
        };

        let timed_prices: Vec<(f64, i64)> =
            prices.iter().map(|p| (p.price, p.timestamp_ms)).collect();
        let decision = decider::decide(
            poly_yes_dec,
            poly_no_dec,
            settlement_ms,
            &account_read,
            &decider_cfg,
            &timed_prices,
        );

        // 6. Execute trade (Stage 4)
        match &decision {
            decider::Decision::Pass(reason) => {
                {
                    let mut st = self.state.write().await;
                    st.no_trade_count += 1;
                    // Compare category only (strip trailing numbers/%)
                    let category = reason
                        .trim_end_matches(|c: char| c.is_ascii_digit() || c == '%' || c == '_');
                    let prev_cat = st
                        .last_no_trade_reason
                        .trim_end_matches(|c: char| c.is_ascii_digit() || c == '%' || c == '_');
                    let changed = category != prev_cat;
                    if changed {
                        st.last_no_trade_reason = reason.clone();
                    }
                    if changed && !reason.contains("cooldown") && !reason.contains("loss_pause") {
                        tracing::info!("[SKIP] {} | BTC=${:.0}", reason, btc_price);
                    }
                }
            }
            decider::Decision::Trade {
                direction,
                size_usdc: _,
                edge,
            } => {
                let cheap_price = match direction {
                    Direction::Up => poly_yes_dec.unwrap_or_else(|| decimal("0.5")),
                    Direction::Down => poly_no_dec.unwrap_or_else(|| decimal("0.5")),
                };

                tracing::info!(
                    "[TRADE] {} @ {:.3} edge={:.0}% BTC=${:.0}",
                    direction.as_str(),
                    cheap_price,
                    (*edge * decimal("100")).round_dp(0),
                    btc_price,
                );

                let order = self
                    .executor
                    .execute(&ExecuteContext {
                        decision: &decision,
                        token_yes: &mkt.token_yes,
                        token_no: &mkt.token_no,
                        poly_yes: poly_yes_dec,
                        poly_no: poly_no_dec,
                        settlement_time_ms: settlement_ms,
                        btc_price,
                    })
                    .await;

                if let Some(order) = order {
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
                        filled_shares: order.filled_shares,
                        cost: order.cost,
                        settlement_time_ms: order.settlement_time_ms,
                        entry_btc_price: order.entry_btc_price,
                        condition_id: mkt.condition_id.clone(),
                        market_slug: mkt.market_slug.clone(),
                    });

                    Self::save_state(&self.log_dir, &self.settler, &self.account).await;

                    let bal = self.account.read().await.balance;
                    Self::write_balance(&self.log_dir, bal).await;

                    // Log to file
                    let log_line = format!(
                        "{},{},{},{:.3},{:.2},{:.1},{:.2}\n",
                        Utc::now().format("%H:%M:%S"),
                        order.direction.as_str(),
                        &order.order_id[..8],
                        order.entry_price,
                        order.cost,
                        (*edge * decimal("100")).round_dp(1),
                        bal,
                    );
                    let trades_path = Path::new(&self.log_dir).join("trades.csv");
                    match tokio::task::spawn_blocking(move || -> std::io::Result<()> {
                        use std::io::Write;

                        let mut file = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(trades_path)?;
                        file.write_all(log_line.as_bytes())
                    })
                    .await
                    {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => tracing::debug!("[LOG] trades.csv write failed: {}", e),
                        Err(e) => tracing::debug!("[LOG] trades.csv task failed: {}", e),
                    }
                }
            }
        }

        Ok(())
    }

    async fn refresh_market(&self) {
        match self.discovery.discover().await {
            Ok(active) => {
                tracing::info!(
                    "[MKT] {} ends {} cid={}",
                    active.market.slug,
                    active.end_date,
                    &active.condition_id[..8.min(active.condition_id.len())]
                );
                *self.market_state.write().await = MarketState {
                    token_yes: active.token_id_yes,
                    token_no: active.token_id_no,
                    condition_id: active.condition_id,
                    market_slug: active.market.slug,
                    settlement_ms: active.end_date.timestamp_millis(),
                };
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
        let discovery = self.discovery.clone();
        let btc_tiebreaker_usd = self.config.strategy.btc_tiebreaker_usd;
        let rpc = data::chainlink::rpc_url(self.config.trading.mode);
        let mode = self.config.trading.mode;
        let redeemer = self.redeemer.clone();
        let log_dir = self.log_dir.clone();

        tokio::spawn(async move {
            let http = reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());
            let mut interval = tokio::time::interval(Duration::from_secs(15));
            // Retry queue: (condition_id, direction_str, attempts_left)
            let mut redeem_queue: Vec<(String, String, u32)> = Vec::new();

            loop {
                interval.tick().await;

                let (results, settlement_btc_price) = if mode.is_paper() {
                    let btc_price = match data::chainlink::fetch_btc_price(&http, &rpc).await {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::debug!("[SETTLE] Chainlink failed: {}, using Coinbase", e);
                            match price_source.latest().await {
                                Some(p) => p,
                                None => continue,
                            }
                        }
                    };

                    (
                        settler
                            .write()
                            .await
                            .check_settlements(btc_price, btc_tiebreaker_usd),
                        Some(btc_price),
                    )
                } else {
                    let mut results = Vec::new();
                    let due = settler.read().await.due_positions();
                    for pos in due {
                        let market = match discovery.fetch_market_by_slug(&pos.market_slug).await {
                            Ok(market) => market,
                            Err(e) => {
                                tracing::warn!(
                                    "[SETTLE] Gamma fetch failed for {}: {}",
                                    pos.market_slug,
                                    e
                                );
                                continue;
                            }
                        };

                        match infer_resolution_state(&market) {
                            Some(ResolutionState::Resolved(winner)) => {
                                tracing::info!(
                                    "[SETTLE] {} resolved -> {} won",
                                    pos.market_slug,
                                    winner.as_str(),
                                );
                                let won = pos.direction == winner;
                                if let Some(result) =
                                    settler.write().await.settle_by_slug(&pos.market_slug, won)
                                {
                                    results.push(result);
                                }
                            }
                            Some(ResolutionState::Pending) => {}
                            None => {
                                tracing::warn!(
                                    "[SETTLE] resolution unclear for {}",
                                    pos.market_slug
                                );
                            }
                        }
                    }
                    let btc_price = price_source.latest().await.unwrap_or(0.0);
                    (results, Some(btc_price))
                };

                if !results.is_empty() {
                    let mut acc = account.write().await;
                    acc.check_daily_reset();
                    for r in &results {
                        acc.record_settlement(r);
                    }

                    tracing::info!(
                        "[BAL] ${:.2} pnl=${:+.2} settled={}",
                        acc.balance,
                        acc.daily_pnl,
                        results.len(),
                    );

                    let bal = acc.balance;
                    drop(acc);

                    Bot::save_state(&log_dir, &settler, &account).await;

                    if let Some(btc_price) = settlement_btc_price {
                        let mut log_lines = String::new();
                        for r in &results {
                            log_lines.push_str(&format!(
                                "{},{},{},{:+.2},{:.0},{:.0}\n",
                                Utc::now().format("%H:%M:%S"),
                                if r.won { "WIN" } else { "LOSS" },
                                r.direction.as_str(),
                                r.pnl.round_dp(2),
                                r.entry_btc_price,
                                btc_price,
                            ));
                        }
                        let trades_path = Path::new(&log_dir).join("trades.csv");
                        match tokio::task::spawn_blocking(move || -> std::io::Result<()> {
                            use std::io::Write;

                            let mut file = std::fs::OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(trades_path)?;
                            file.write_all(log_lines.as_bytes())
                        })
                        .await
                        {
                            Ok(Ok(())) => {}
                            Ok(Err(e)) => tracing::debug!("[LOG] trades.csv write failed: {}", e),
                            Err(e) => tracing::debug!("[LOG] trades.csv task failed: {}", e),
                        }
                    }

                    Bot::write_balance(&log_dir, bal).await;

                    // Queue winning positions for on-chain redeem
                    for r in &results {
                        if r.won && !r.condition_id.is_empty() {
                            redeem_queue.push((
                                r.condition_id.clone(),
                                r.direction.as_str().to_string(),
                                10, // max retries (~5 min at 30s intervals)
                            ));
                        }
                    }
                }

                // Process redeem retry queue
                if let Some(ref redeemer) = redeemer {
                    let mut still_pending = Vec::new();
                    for (cid, dir, attempts) in redeem_queue.drain(..) {
                        match redeemer.has_redeemable_position(&cid).await {
                            Ok(true) => match redeemer.redeem(&cid).await {
                                Ok(tx) => {
                                    tracing::info!("[REDEEM] {} tx={}", dir, tx);
                                }
                                Err(e) => {
                                    tracing::warn!("[REDEEM] {} failed: {}", dir, e);
                                }
                            },
                            Ok(false) if attempts > 1 => {
                                tracing::debug!(
                                    "[REDEEM] {} not redeemable yet, {} retries left",
                                    dir,
                                    attempts - 1
                                );
                                still_pending.push((cid, dir, attempts - 1));
                            }
                            Ok(false) => {
                                tracing::debug!(
                                    "[REDEEM] {} no redeemable position, dropping",
                                    dir
                                );
                            }
                            Err(e) if attempts > 1 => {
                                tracing::debug!(
                                    "[REDEEM] {} check failed: {}, {} retries left",
                                    dir,
                                    e,
                                    attempts - 1
                                );
                                still_pending.push((cid, dir, attempts - 1));
                            }
                            Err(e) => {
                                tracing::warn!("[REDEEM] {} check failed, dropping: {}", dir, e);
                            }
                        }
                    }
                    redeem_queue = still_pending;
                }
            }
        })
    }
}

// ─── Main ───

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

    // Handle subcommands before full bot init
    if std::env::args().any(|a| a == "--derive-keys") {
        return derive_api_keys().await;
    }
    if std::env::args().any(|a| a == "--redeem-all") {
        return redeem_all().await;
    }
    if let Some(slug) = std::env::args().skip_while(|a| a != "--redeem").nth(1) {
        return redeem_one(&slug).await;
    }

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

    let file_appender = tracing_appender::rolling::never(&log_dir, "bot.log");
    let (file_writer, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(file_writer.and(std::io::stderr))
        .with_ansi(false)
        .init();
    tracing::info!("polybot v{}", env!("CARGO_PKG_VERSION"));

    if config.trading.mode.is_live() && config.is_default_non_trading() {
        tracing::warn!(
            "[INIT] Running live mode with default config values; review config.json before trading"
        );
    }

    // Validate credentials for live mode
    if config.trading.mode.is_live() && config.trading.private_key.expose_secret().is_empty() {
        anyhow::bail!("PRIVATE_KEY not set in .env — required for live trading");
    }

    let mut bot = Bot::new(config, log_dir).await?;
    bot.run().await?;

    Ok(())
}

/// Redeem all winning outcome tokens for recent markets in the series.
async fn redeem_all() -> Result<()> {
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
    let rpc = data::chainlink::rpc_url(mode);
    let redeemer = data::polymarket::CtfRedeemer::new(private_key, rpc);

    let gamma_url = &config.polyclob.gamma_api_url;
    let http = reqwest::Client::new();

    // Scan past 24h of 5-minute windows (288 windows)
    let now_ts = chrono::Utc::now().timestamp();
    let base_ts = (now_ts / WINDOW_SECS) * WINDOW_SECS;
    let mut condition_ids: Vec<(String, String)> = Vec::new(); // (condition_id, slug)

    for i in 0..WINDOWS_PER_DAY {
        let ts = base_ts - i * WINDOW_SECS;
        let slug = format!("{}-{}", data::market_discovery::SERIES_ID, ts);
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
        tokio::time::sleep(Duration::from_secs(5)).await;
    }

    eprintln!("\nDone: {} redeemed, {} failed", success, failed);
    Ok(())
}

async fn redeem_one(slug: &str) -> Result<()> {
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
    let rpc = data::chainlink::rpc_url(mode);
    let redeemer = data::polymarket::CtfRedeemer::new(private_key, rpc);

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
        let state = data::market_discovery::infer_resolution_state(
            &serde_json::from_value::<data::market_discovery::GammaMarket>(m.clone()).ok()?,
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

/// Derive CLOB API credentials from wallet using the SDK and write to .env
async fn derive_api_keys() -> Result<()> {
    use polymarket_client_sdk::auth::{LocalSigner, Signer as _};
    use polymarket_client_sdk::clob;
    use polymarket_client_sdk::POLYGON;
    use secrecy::ExposeSecret;
    use std::str::FromStr;

    eprintln!("🔑 Deriving Polymarket CLOB API credentials...");

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

    // Create or derive API key via SDK
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
