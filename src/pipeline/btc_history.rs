//! BTC window history for dynamic fair value calculation

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BtcWindow {
    pub start_time_ms: i64,
    pub end_time_ms: i64,
    pub start_price: Decimal,
    pub end_price: Decimal,
    pub up_won: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BtcHistory {
    windows: VecDeque<BtcWindow>,
    #[serde(default = "default_max_windows")]
    max_windows: usize,
}

#[allow(dead_code)]
fn default_max_windows() -> usize {
    1000
}

impl Default for BtcHistory {
    fn default() -> Self {
        Self::new(1000)
    }
}

#[allow(dead_code)]
impl BtcHistory {
    pub(crate) fn new(max_windows: usize) -> Self {
        Self {
            windows: VecDeque::with_capacity(max_windows),
            max_windows,
        }
    }

    pub(crate) fn record_window(
        &mut self,
        start_price: Decimal,
        end_price: Decimal,
        start_time_ms: i64,
        end_time_ms: i64,
    ) {
        let up_won = end_price > start_price;
        let window = BtcWindow {
            start_time_ms,
            end_time_ms,
            start_price,
            end_price,
            up_won,
        };

        if self.windows.len() >= self.max_windows {
            self.windows.pop_front();
        }
        self.windows.push_back(window);
    }

    pub(crate) fn dynamic_fair_value(&self, min_samples: usize) -> Option<Decimal> {
        if self.windows.len() < min_samples {
            return None;
        }

        let up_count = self.windows.iter().filter(|w| w.up_won).count();
        let total = self.windows.len();

        Some(Decimal::from(up_count) / Decimal::from(total))
    }

    pub(crate) fn len(&self) -> usize {
        self.windows.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    pub(crate) fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub(crate) fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(value: &str) -> Decimal {
        Decimal::from_str_exact(value).expect("valid decimal")
    }

    #[test]
    fn test_record_window() {
        let mut history = BtcHistory::new(100);

        history.record_window(d("100"), d("105"), 0, 300000);

        assert_eq!(history.len(), 1);
        assert!(history.windows[0].up_won);
    }

    #[test]
    fn test_record_window_down() {
        let mut history = BtcHistory::new(100);

        history.record_window(d("100"), d("95"), 0, 300000);

        assert_eq!(history.len(), 1);
        assert!(!history.windows[0].up_won);
    }

    #[test]
    fn test_dynamic_fv_returns_none_when_insufficient_samples() {
        let mut history = BtcHistory::new(100);

        for i in 0..10 {
            history.record_window(d("100"), d("101"), i * 300000, (i + 1) * 300000);
        }

        assert!(history.dynamic_fair_value(20).is_none());
    }

    #[test]
    fn test_dynamic_fv_computes_correct_ratio() {
        let mut history = BtcHistory::new(100);

        // 6 up, 4 down -> FV = 0.60
        for i in 0..6 {
            history.record_window(d("100"), d("101"), i * 300000, (i + 1) * 300000);
        }
        for i in 6..10 {
            history.record_window(d("100"), d("99"), i * 300000, (i + 1) * 300000);
        }

        let fv = history.dynamic_fair_value(10).unwrap();
        assert_eq!(fv, d("0.6"));
    }

    #[test]
    fn test_max_windows_eviction() {
        let mut history = BtcHistory::new(5);

        for i in 0..10 {
            history.record_window(d("100"), d("101"), i * 300000, (i + 1) * 300000);
        }

        assert_eq!(history.len(), 5);
        // First window should be evicted, last should start at i=5
        assert_eq!(history.windows.front().unwrap().start_time_ms, 5 * 300000);
    }

    #[test]
    fn test_to_json_from_json_roundtrip() {
        let mut history = BtcHistory::new(100);

        history.record_window(d("100"), d("105"), 0, 300000);
        history.record_window(d("105"), d("100"), 300000, 600000);

        let json = history.to_json().unwrap();
        let restored = BtcHistory::from_json(&json).unwrap();

        assert_eq!(restored.len(), 2);
        assert!(restored.windows[0].up_won);
        assert!(!restored.windows[1].up_won);
    }
}
