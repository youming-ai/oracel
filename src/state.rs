use std::sync::Arc;

pub(crate) struct BotState {
    pub last_no_trade_reason: String,
    pub last_idle_reason: String,
    pub fak_rejections: u32,
    pub fak_market_ms: i64,
    pub last_fak_rejection_ms: i64,
}

impl BotState {
    pub(crate) fn new() -> Self {
        Self {
            last_no_trade_reason: String::new(),
            last_idle_reason: String::new(),
            fak_rejections: 0,
            fak_market_ms: 0,
            last_fak_rejection_ms: 0,
        }
    }

    pub(crate) fn log_idle_change(&mut self, reason: &str, detail: &str) {
        if self.last_idle_reason != reason {
            self.last_idle_reason = reason.to_string();
            tracing::debug!("[IDLE] {} | {}", reason, detail);
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct MarketState {
    pub token_yes: Arc<str>,
    pub token_no: Arc<str>,
    pub condition_id: Arc<str>,
    pub market_slug: Arc<str>,
    pub settlement_ms: i64,
}

impl MarketState {
    pub(crate) fn is_ready(&self) -> bool {
        !self.token_yes.is_empty() && !self.token_no.is_empty()
    }
}
