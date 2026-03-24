//! Multi-timeframe momentum computation

use crate::pipeline::price_source::PriceTick;
use crate::pipeline::signal::Direction;
use rust_decimal::Decimal;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct MomentumSignal {
    pub short: Decimal,
    pub medium: Decimal,
    pub long: Decimal,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MomentumMode {
    AllAligned,
}

#[allow(dead_code)]
pub(crate) fn compute_momentum(prices: &[PriceTick], window_secs: u64) -> Decimal {
    if prices.len() < 2 {
        return Decimal::ZERO;
    }

    let now_ms = prices.last().map(|p| p.timestamp_ms).unwrap_or(0);
    let cutoff_ms = now_ms - (window_secs as i64 * 1000);

    let start_price = prices
        .iter()
        .find(|p| p.timestamp_ms >= cutoff_ms)
        .map(|p| p.price)
        .unwrap_or_else(|| prices.first().map(|p| p.price).unwrap_or(Decimal::ZERO));

    let end_price = prices.last().map(|p| p.price).unwrap_or(Decimal::ZERO);

    if start_price == Decimal::ZERO {
        return Decimal::ZERO;
    }

    (end_price - start_price) / start_price
}

#[allow(dead_code)]
pub(crate) fn compute_multi_frame_momentum(
    prices: &[PriceTick],
    short_secs: u64,
    medium_secs: u64,
    long_secs: u64,
) -> MomentumSignal {
    MomentumSignal {
        short: compute_momentum(prices, short_secs),
        medium: compute_momentum(prices, medium_secs),
        long: compute_momentum(prices, long_secs),
    }
}

#[allow(dead_code)]
pub(crate) fn momentum_aligned(
    signal: &MomentumSignal,
    direction: Direction,
    mode: MomentumMode,
) -> bool {
    match mode {
        MomentumMode::AllAligned => {
            let all_up = signal.short > Decimal::ZERO
                && signal.medium > Decimal::ZERO
                && signal.long > Decimal::ZERO;
            let all_down = signal.short < Decimal::ZERO
                && signal.medium < Decimal::ZERO
                && signal.long < Decimal::ZERO;

            match direction {
                Direction::Up => all_up,
                Direction::Down => all_down,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::test_helpers::d;

    fn make_prices() -> Vec<PriceTick> {
        vec![
            PriceTick {
                price: d("100"),
                timestamp_ms: 0,
            },
            PriceTick {
                price: d("101"),
                timestamp_ms: 30000,
            },
            PriceTick {
                price: d("102"),
                timestamp_ms: 60000,
            },
            PriceTick {
                price: d("103"),
                timestamp_ms: 90000,
            },
            PriceTick {
                price: d("104"),
                timestamp_ms: 120000,
            },
            PriceTick {
                price: d("105"),
                timestamp_ms: 150000,
            },
            PriceTick {
                price: d("106"),
                timestamp_ms: 180000,
            },
        ]
    }

    #[test]
    fn test_compute_momentum_positive() {
        let prices = make_prices();
        let momentum = compute_momentum(&prices, 180);
        assert!(momentum > Decimal::ZERO);
    }

    #[test]
    fn test_compute_momentum_negative() {
        let prices: Vec<PriceTick> = vec![
            PriceTick {
                price: d("100"),
                timestamp_ms: 0,
            },
            PriceTick {
                price: d("99"),
                timestamp_ms: 60000,
            },
        ];
        let momentum = compute_momentum(&prices, 60);
        assert!(momentum < Decimal::ZERO);
    }

    #[test]
    fn test_compute_multi_frame_momentum() {
        let prices = make_prices();
        let signal = compute_multi_frame_momentum(&prices, 30, 60, 180);

        assert!(signal.short > Decimal::ZERO);
        assert!(signal.medium > Decimal::ZERO);
        assert!(signal.long > Decimal::ZERO);
    }

    #[test]
    fn test_momentum_aligned_all_up() {
        let signal = MomentumSignal {
            short: d("0.01"),
            medium: d("0.02"),
            long: d("0.03"),
        };

        assert!(momentum_aligned(
            &signal,
            Direction::Up,
            MomentumMode::AllAligned
        ));
        assert!(!momentum_aligned(
            &signal,
            Direction::Down,
            MomentumMode::AllAligned
        ));
    }

    #[test]
    fn test_momentum_aligned_all_down() {
        let signal = MomentumSignal {
            short: d("-0.01"),
            medium: d("-0.02"),
            long: d("-0.03"),
        };

        assert!(momentum_aligned(
            &signal,
            Direction::Down,
            MomentumMode::AllAligned
        ));
        assert!(!momentum_aligned(
            &signal,
            Direction::Up,
            MomentumMode::AllAligned
        ));
    }

    #[test]
    fn test_momentum_not_aligned_mixed() {
        let signal = MomentumSignal {
            short: d("0.01"),
            medium: d("-0.02"),
            long: d("0.03"),
        };

        assert!(!momentum_aligned(
            &signal,
            Direction::Up,
            MomentumMode::AllAligned
        ));
        assert!(!momentum_aligned(
            &signal,
            Direction::Down,
            MomentumMode::AllAligned
        ));
    }

    #[test]
    fn test_momentum_insufficient_data() {
        let prices = vec![PriceTick {
            price: d("100"),
            timestamp_ms: 0,
        }];
        let momentum = compute_momentum(&prices, 60);
        assert_eq!(momentum, Decimal::ZERO);
    }
}
