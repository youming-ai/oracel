//! Binance Prediction bot configuration.

use rust_decimal::Decimal;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

fn dec(value: &'static str) -> Decimal {
    Decimal::from_str_exact(value).expect("valid Decimal default")
}

mod defaults {
    use super::*;

    pub fn paper_starting_balance() -> Decimal {
        dec("100")
    }
    pub const fn allow_uncalibrated_model_live() -> bool {
        false
    }
    pub fn secret() -> SecretString {
        SecretString::new(String::new().into())
    }
    pub fn symbol() -> String {
        "BTCUSDT".to_string()
    }
    pub const fn prediction_api_timeout_ms() -> u64 {
        10_000
    }
    pub const fn prediction_market_duration_ms() -> u64 {
        300_000
    }
    pub const fn prediction_discovery_limit() -> usize {
        50
    }
    pub const fn prediction_quote_slippage_bps() -> u32 {
        25
    }
    pub const fn prediction_order_reconciliation_attempts() -> u32 {
        5
    }
    pub const fn prediction_order_reconciliation_delay_ms() -> u64 {
        1_000
    }
    pub fn position_size_usdt() -> Decimal {
        // Binance Prediction MARKET/FOK orders require roughly 1.5 USDT minimum.
        dec("2")
    }
    pub const fn min_entry_ttl_ms() -> u64 {
        75_000
    }
    pub const fn max_entry_ttl_ms() -> u64 {
        150_000
    }
    pub fn min_normalized_move() -> Decimal {
        dec("0.60")
    }
    pub fn min_net_edge() -> Decimal {
        dec("0.05")
    }
    pub fn model_uncertainty() -> Decimal {
        dec("0.03")
    }
    pub fn fee_buffer() -> Decimal {
        dec("0.02")
    }
    pub fn max_spread() -> Decimal {
        dec("0.03")
    }
    pub fn min_depth_multiple() -> Decimal {
        dec("5")
    }
    pub const fn volatility_lookback_secs() -> u64 {
        900
    }
    pub const fn min_volatility_samples() -> usize {
        120
    }
    pub const fn order_book_stale_threshold_ms() -> i64 {
        8_000
    }
    pub const fn max_unsettled_positions() -> usize {
        2
    }
    pub const fn circuit_breaker_window() -> u32 {
        50
    }
    pub fn circuit_breaker_min_win_rate() -> Decimal {
        dec("0.05")
    }
    pub fn daily_loss_limit_usdt() -> Decimal {
        dec("5")
    }
    pub const fn max_consecutive_losses() -> u32 {
        3
    }
    pub const fn loss_cooldown_ms() -> i64 {
        1_800_000
    }
    pub const fn max_trades_per_day() -> u32 {
        8
    }
    pub const fn signal_interval_ms() -> u64 {
        1_000
    }
    pub const fn status_interval_ms() -> u64 {
        10_000
    }
    pub const fn market_refresh_secs() -> u64 {
        15
    }
    pub const fn settlement_check_secs() -> u64 {
        15
    }
    pub const fn stale_threshold_ms() -> i64 {
        3_000
    }
    pub const fn price_buffer_max() -> usize {
        2_000
    }
    pub const fn price_buffer_min_ticks() -> usize {
        120
    }
    pub const fn trade_log_flush_secs() -> u64 {
        30
    }
    pub const fn shutdown_timeout_secs() -> u64 {
        10
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub trading: TradingConfig,
    #[serde(default)]
    pub binance_prediction: BinancePredictionConfig,
    #[serde(default)]
    pub strategy: StrategyConfig,
    #[serde(default)]
    pub risk: RiskConfig,
    #[serde(default)]
    pub polling: PollingConfig,
    #[serde(default)]
    pub price_source: PriceSourceConfig,
    #[serde(default)]
    pub misc: MiscConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TradingMode {
    #[default]
    Paper,
    Live,
}

impl TradingMode {
    pub const fn is_paper(self) -> bool {
        matches!(self, Self::Paper)
    }

    pub const fn is_live(self) -> bool {
        matches!(self, Self::Live)
    }
}

impl std::fmt::Display for TradingMode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Paper => write!(formatter, "paper"),
            Self::Live => write!(formatter, "live"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TradingConfig {
    #[serde(default)]
    pub mode: TradingMode,
    #[serde(
        default = "defaults::paper_starting_balance",
        with = "rust_decimal::serde::float"
    )]
    pub paper_starting_balance: Decimal,
    /// Explicit acknowledgement required until the probability model is calibrated.
    #[serde(default = "defaults::allow_uncalibrated_model_live")]
    pub allow_uncalibrated_model_live: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaymentAccount {
    #[default]
    Spot,
    Funding,
}

impl PaymentAccount {
    pub const fn as_api_str(self) -> &'static str {
        match self {
            Self::Spot => "SPOT",
            Self::Funding => "FUNDING",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FundingSource {
    #[default]
    Cex,
    Mpc,
}

impl FundingSource {
    pub const fn as_api_str(self) -> &'static str {
        match self {
            Self::Cex => "CEX",
            Self::Mpc => "MPC",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BinancePredictionConfig {
    /// Loaded from BINANCE_API_KEY; never serialized to config.toml.
    #[serde(skip, default = "defaults::secret")]
    pub api_key: SecretString,
    /// Loaded from BINANCE_API_SECRET; never serialized to config.toml.
    #[serde(skip, default = "defaults::secret")]
    pub api_secret: SecretString,
    /// Optional explicit wallet selection. Both fields must be supplied together.
    #[serde(default)]
    pub wallet_id: Option<String>,
    #[serde(default)]
    pub wallet_address: Option<String>,
    #[serde(default)]
    pub payment_account: PaymentAccount,
    #[serde(default)]
    pub funding_source: FundingSource,
    #[serde(default = "defaults::prediction_api_timeout_ms")]
    pub api_timeout_ms: u64,
    #[serde(default = "defaults::prediction_market_duration_ms")]
    pub market_duration_ms: u64,
    #[serde(default = "defaults::prediction_discovery_limit")]
    pub market_discovery_limit: usize,
    #[serde(default = "defaults::prediction_quote_slippage_bps")]
    pub quote_slippage_bps: u32,
    #[serde(default = "defaults::prediction_order_reconciliation_attempts")]
    pub order_reconciliation_attempts: u32,
    #[serde(default = "defaults::prediction_order_reconciliation_delay_ms")]
    pub order_reconciliation_delay_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrategyConfig {
    #[serde(
        default = "defaults::position_size_usdt",
        with = "rust_decimal::serde::float"
    )]
    pub position_size_usdt: Decimal,
    #[serde(default = "defaults::min_entry_ttl_ms")]
    pub min_entry_ttl_ms: u64,
    #[serde(default = "defaults::max_entry_ttl_ms")]
    pub max_entry_ttl_ms: u64,
    #[serde(
        default = "defaults::min_normalized_move",
        with = "rust_decimal::serde::float"
    )]
    pub min_normalized_move: Decimal,
    #[serde(
        default = "defaults::min_net_edge",
        with = "rust_decimal::serde::float"
    )]
    pub min_net_edge: Decimal,
    #[serde(
        default = "defaults::model_uncertainty",
        with = "rust_decimal::serde::float"
    )]
    pub model_uncertainty: Decimal,
    #[serde(default = "defaults::fee_buffer", with = "rust_decimal::serde::float")]
    pub fee_buffer: Decimal,
    #[serde(default = "defaults::max_spread", with = "rust_decimal::serde::float")]
    pub max_spread: Decimal,
    #[serde(
        default = "defaults::min_depth_multiple",
        with = "rust_decimal::serde::float"
    )]
    pub min_depth_multiple: Decimal,
    #[serde(default = "defaults::volatility_lookback_secs")]
    pub volatility_lookback_secs: u64,
    #[serde(default = "defaults::min_volatility_samples")]
    pub min_volatility_samples: usize,
    #[serde(default = "defaults::order_book_stale_threshold_ms")]
    pub order_book_stale_threshold_ms: i64,
    #[serde(default = "defaults::max_unsettled_positions")]
    pub max_unsettled_positions: usize,
    #[serde(default = "defaults::circuit_breaker_window")]
    pub circuit_breaker_window: u32,
    #[serde(
        default = "defaults::circuit_breaker_min_win_rate",
        with = "rust_decimal::serde::float"
    )]
    pub circuit_breaker_min_win_rate: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RiskConfig {
    #[serde(
        default = "defaults::daily_loss_limit_usdt",
        with = "rust_decimal::serde::float"
    )]
    pub daily_loss_limit_usdt: Decimal,
    #[serde(default = "defaults::max_consecutive_losses")]
    pub max_consecutive_losses: u32,
    #[serde(default = "defaults::loss_cooldown_ms")]
    pub loss_cooldown_ms: i64,
    #[serde(default = "defaults::max_trades_per_day")]
    pub max_trades_per_day: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PollingConfig {
    #[serde(default = "defaults::signal_interval_ms")]
    pub signal_interval_ms: u64,
    #[serde(default = "defaults::status_interval_ms")]
    pub status_interval_ms: u64,
    #[serde(default = "defaults::market_refresh_secs")]
    pub market_refresh_secs: u64,
    #[serde(default = "defaults::settlement_check_secs")]
    pub settlement_check_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PriceSourceConfig {
    #[serde(default = "defaults::symbol")]
    pub symbol: String,
    #[serde(default = "defaults::stale_threshold_ms")]
    pub stale_threshold_ms: i64,
    #[serde(default = "defaults::price_buffer_max")]
    pub buffer_max: usize,
    #[serde(default = "defaults::price_buffer_min_ticks")]
    pub buffer_min_ticks: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MiscConfig {
    #[serde(default = "defaults::trade_log_flush_secs")]
    pub trade_log_flush_secs: u64,
    #[serde(default = "defaults::shutdown_timeout_secs")]
    pub shutdown_timeout_secs: u64,
}

impl Default for TradingConfig {
    fn default() -> Self {
        Self {
            mode: TradingMode::default(),
            paper_starting_balance: defaults::paper_starting_balance(),
            allow_uncalibrated_model_live: defaults::allow_uncalibrated_model_live(),
        }
    }
}

impl Default for BinancePredictionConfig {
    fn default() -> Self {
        Self {
            api_key: defaults::secret(),
            api_secret: defaults::secret(),
            wallet_id: None,
            wallet_address: None,
            payment_account: PaymentAccount::default(),
            funding_source: FundingSource::default(),
            api_timeout_ms: defaults::prediction_api_timeout_ms(),
            market_duration_ms: defaults::prediction_market_duration_ms(),
            market_discovery_limit: defaults::prediction_discovery_limit(),
            quote_slippage_bps: defaults::prediction_quote_slippage_bps(),
            order_reconciliation_attempts: defaults::prediction_order_reconciliation_attempts(),
            order_reconciliation_delay_ms: defaults::prediction_order_reconciliation_delay_ms(),
        }
    }
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            position_size_usdt: defaults::position_size_usdt(),
            min_entry_ttl_ms: defaults::min_entry_ttl_ms(),
            max_entry_ttl_ms: defaults::max_entry_ttl_ms(),
            min_normalized_move: defaults::min_normalized_move(),
            min_net_edge: defaults::min_net_edge(),
            model_uncertainty: defaults::model_uncertainty(),
            fee_buffer: defaults::fee_buffer(),
            max_spread: defaults::max_spread(),
            min_depth_multiple: defaults::min_depth_multiple(),
            volatility_lookback_secs: defaults::volatility_lookback_secs(),
            min_volatility_samples: defaults::min_volatility_samples(),
            order_book_stale_threshold_ms: defaults::order_book_stale_threshold_ms(),
            max_unsettled_positions: defaults::max_unsettled_positions(),
            circuit_breaker_window: defaults::circuit_breaker_window(),
            circuit_breaker_min_win_rate: defaults::circuit_breaker_min_win_rate(),
        }
    }
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            daily_loss_limit_usdt: defaults::daily_loss_limit_usdt(),
            max_consecutive_losses: defaults::max_consecutive_losses(),
            loss_cooldown_ms: defaults::loss_cooldown_ms(),
            max_trades_per_day: defaults::max_trades_per_day(),
        }
    }
}

impl Default for PollingConfig {
    fn default() -> Self {
        Self {
            signal_interval_ms: defaults::signal_interval_ms(),
            status_interval_ms: defaults::status_interval_ms(),
            market_refresh_secs: defaults::market_refresh_secs(),
            settlement_check_secs: defaults::settlement_check_secs(),
        }
    }
}

impl Default for PriceSourceConfig {
    fn default() -> Self {
        Self {
            symbol: defaults::symbol(),
            stale_threshold_ms: defaults::stale_threshold_ms(),
            buffer_max: defaults::price_buffer_max(),
            buffer_min_ticks: defaults::price_buffer_min_ticks(),
        }
    }
}

impl Default for MiscConfig {
    fn default() -> Self {
        Self {
            trade_log_flush_secs: defaults::trade_log_flush_secs(),
            shutdown_timeout_secs: defaults::shutdown_timeout_secs(),
        }
    }
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        let mut config: Self = toml::from_str(&content)?;
        if let Ok(value) = std::env::var("BINANCE_API_KEY") {
            config.binance_prediction.api_key = SecretString::new(value.into());
        }
        if let Ok(value) = std::env::var("BINANCE_API_SECRET") {
            config.binance_prediction.api_secret = SecretString::new(value.into());
        }
        // Treat a blank env var (e.g. from copying .env.example) as unset, so it
        // does not become a half-configured wallet selection that fails validation.
        if let Some(value) = env_nonempty("BINANCE_PREDICTION_WALLET_ID") {
            config.binance_prediction.wallet_id = Some(value);
        }
        if let Some(value) = env_nonempty("BINANCE_PREDICTION_WALLET_ADDRESS") {
            config.binance_prediction.wallet_address = Some(value);
        }
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        // Atomic write: a crash mid-write must not leave a truncated config.toml.
        let contents = toml::to_string_pretty(self)?;
        let temporary = path.with_extension("toml.tmp");
        fs::write(&temporary, contents)?;
        fs::rename(&temporary, path)?;
        Ok(())
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        let zero = Decimal::ZERO;
        let one = Decimal::ONE;
        let prediction = &self.binance_prediction;
        let strategy = &self.strategy;

        if self.trading.paper_starting_balance <= zero {
            anyhow::bail!("trading.paper_starting_balance must be > 0");
        }
        if self.trading.mode.is_live() && !self.trading.allow_uncalibrated_model_live {
            anyhow::bail!(
                "live mode requires trading.allow_uncalibrated_model_live = true until the model is calibrated"
            );
        }
        if prediction.api_key.expose_secret().trim().is_empty()
            || prediction.api_secret.expose_secret().trim().is_empty()
        {
            anyhow::bail!("BINANCE_API_KEY and BINANCE_API_SECRET must be set in .env");
        }
        if prediction.wallet_id.is_some() != prediction.wallet_address.is_some() {
            anyhow::bail!(
                "binance_prediction.wallet_id and wallet_address must be configured together"
            );
        }
        if prediction.api_timeout_ms == 0
            || prediction.market_duration_ms == 0
            || prediction.market_discovery_limit == 0
            || prediction.order_reconciliation_attempts == 0
            || prediction.order_reconciliation_delay_ms == 0
        {
            anyhow::bail!("Binance Prediction timeouts, limits, and retry counts must be > 0");
        }
        if !(1..=10_000).contains(&prediction.quote_slippage_bps) {
            anyhow::bail!("binance_prediction.quote_slippage_bps must be in [1, 10000]");
        }
        if !is_valid_binance_symbol(&self.price_source.symbol) {
            anyhow::bail!(
                "price_source.symbol must match Binance format like BTCUSDT (got {})",
                self.price_source.symbol
            );
        }
        if self.price_source.stale_threshold_ms <= 0
            || self.price_source.buffer_max == 0
            || self.price_source.buffer_min_ticks == 0
            || self.price_source.buffer_min_ticks > self.price_source.buffer_max
        {
            anyhow::bail!("price source staleness and buffer settings are invalid");
        }
        if self.polling.signal_interval_ms == 0
            || self.polling.status_interval_ms == 0
            || self.polling.market_refresh_secs == 0
            || self.polling.settlement_check_secs == 0
            || self.misc.trade_log_flush_secs == 0
            || self.misc.shutdown_timeout_secs == 0
        {
            anyhow::bail!("polling and shutdown intervals must be > 0");
        }
        if strategy.position_size_usdt < Decimal::from(2) {
            anyhow::bail!(
                "strategy.position_size_usdt must be >= 2 for Binance Prediction MARKET/FOK orders"
            );
        }
        if strategy.min_entry_ttl_ms == 0
            || strategy.min_entry_ttl_ms >= strategy.max_entry_ttl_ms
            || strategy.max_entry_ttl_ms >= prediction.market_duration_ms
        {
            anyhow::bail!(
                "strategy entry TTLs must satisfy 0 < min < max < binance_prediction.market_duration_ms"
            );
        }
        if strategy.min_normalized_move <= zero {
            anyhow::bail!("strategy.min_normalized_move must be > 0");
        }
        for (name, value) in [
            ("min_net_edge", strategy.min_net_edge),
            ("model_uncertainty", strategy.model_uncertainty),
            ("fee_buffer", strategy.fee_buffer),
            ("max_spread", strategy.max_spread),
        ] {
            if value < zero || value >= one {
                anyhow::bail!("strategy.{name} must be in [0, 1)");
            }
        }
        if strategy.min_net_edge + strategy.model_uncertainty + strategy.fee_buffer >= one {
            anyhow::bail!("strategy edge, uncertainty, and fee buffers must sum to < 1");
        }
        if strategy.min_depth_multiple < one
            || strategy.volatility_lookback_secs == 0
            || strategy.min_volatility_samples == 0
            || strategy.min_volatility_samples as u64 >= strategy.volatility_lookback_secs
            || strategy.order_book_stale_threshold_ms <= 0
            || strategy.max_unsettled_positions == 0
        {
            anyhow::bail!("strategy depth, volatility, or position settings are invalid");
        }
        if strategy.circuit_breaker_min_win_rate < zero
            || strategy.circuit_breaker_min_win_rate > one
            || strategy.circuit_breaker_window > 200
        {
            anyhow::bail!("strategy circuit breaker settings are invalid");
        }
        if self.risk.daily_loss_limit_usdt < zero
            || self.risk.max_consecutive_losses == 0
            || self.risk.loss_cooldown_ms <= 0
            || self.risk.max_trades_per_day == 0
        {
            anyhow::bail!("risk settings are invalid");
        }
        Ok(())
    }
}

fn is_valid_binance_symbol(symbol: &str) -> bool {
    !symbol.is_empty()
        && symbol
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    fn configured() -> Config {
        let mut config = Config::default();
        config.binance_prediction.api_key = SecretString::new("test-key".into());
        config.binance_prediction.api_secret = SecretString::new("test-secret".into());
        config
    }

    #[test]
    fn defaults_require_binance_credentials() {
        let error = Config::default()
            .validate()
            .expect_err("credentials required");
        assert!(error.to_string().contains("BINANCE_API_KEY"));
    }

    #[test]
    fn configured_defaults_validate() {
        assert!(configured().validate().is_ok());
    }

    #[test]
    fn live_requires_explicit_model_acknowledgement() {
        let mut config = configured();
        config.trading.mode = TradingMode::Live;
        assert!(config.validate().is_err());
        config.trading.allow_uncalibrated_model_live = true;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn rejects_unknown_sections() {
        let result = toml::from_str::<Config>("[legacy]\nvalue = 'x'\n");
        assert!(result.is_err());
    }

    #[test]
    fn rejects_invalid_market_order_size() {
        let mut config = configured();
        config.strategy.position_size_usdt = dec("1.5");
        assert!(config.validate().is_err());
    }

    #[test]
    fn validates_wallet_selection_as_a_pair() {
        let mut config = configured();
        config.binance_prediction.wallet_id = Some("wallet".into());
        assert!(config.validate().is_err());
        config.binance_prediction.wallet_address = Some("0xabc".into());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn environment_credentials_are_not_serialized() {
        let config = configured();
        let serialized = toml::to_string_pretty(&config).unwrap();
        assert!(!serialized.contains(config.binance_prediction.api_key.expose_secret()));
        assert!(!serialized.contains(config.binance_prediction.api_secret.expose_secret()));
    }
}
