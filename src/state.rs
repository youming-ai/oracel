use binance_5m_bot::data::binance_prediction::ActivePredictionMarket;

pub(crate) struct BotState {
    pub last_no_trade_reason: String,
    pub last_idle_reason: String,
    pub execution_halted: bool,
}

impl BotState {
    pub(crate) fn new() -> Self {
        Self {
            last_no_trade_reason: String::new(),
            last_idle_reason: String::new(),
            execution_halted: false,
        }
    }

    pub(crate) fn log_idle_change(&mut self, reason: &str, detail: &str) {
        if self.last_idle_reason != reason {
            self.last_idle_reason = reason.to_string();
            tracing::debug!("[IDLE] {reason} | {detail}");
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct MarketState {
    pub active: Option<ActivePredictionMarket>,
}
