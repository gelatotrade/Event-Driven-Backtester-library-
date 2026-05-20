use super::Strategy;
use crate::data::DataHandler;
use crate::events::{MarketEvent, SignalDirection, SignalEvent};
use crate::types::Symbol;

/// Classic moving-average crossover.
///
/// Goes long when the fast SMA crosses above the slow SMA and short on the
/// opposite cross. Useful as a reference strategy for tests and demos.
pub struct MovingAverageCrossover {
    symbol: Symbol,
    fast: usize,
    slow: usize,
    last_direction: Option<SignalDirection>,
    name: String,
}

impl MovingAverageCrossover {
    pub fn new(symbol: impl Into<Symbol>, fast: usize, slow: usize) -> Self {
        assert!(fast < slow, "fast window must be shorter than slow window");
        let symbol = symbol.into();
        let name = format!("MA({fast},{slow})/{symbol}");
        Self {
            symbol,
            fast,
            slow,
            last_direction: None,
            name,
        }
    }
}

impl Strategy for MovingAverageCrossover {
    fn name(&self) -> &str {
        &self.name
    }

    fn on_market(&mut self, event: &MarketEvent, data: &dyn DataHandler) -> Vec<SignalEvent> {
        if event.bar.symbol != self.symbol {
            return Vec::new();
        }
        let hist = data.history(&self.symbol, self.slow);
        if hist.len() < self.slow {
            return Vec::new();
        }
        let slow_avg: f64 = hist.iter().map(|b| b.close).sum::<f64>() / self.slow as f64;
        let fast_start = hist.len() - self.fast;
        let fast_avg: f64 =
            hist[fast_start..].iter().map(|b| b.close).sum::<f64>() / self.fast as f64;

        let new_dir = if fast_avg > slow_avg {
            SignalDirection::Long
        } else {
            SignalDirection::Short
        };

        if Some(new_dir) == self.last_direction {
            return Vec::new();
        }
        self.last_direction = Some(new_dir);

        vec![SignalEvent {
            timestamp: event.timestamp,
            symbol: self.symbol.clone(),
            direction: new_dir,
            strength: 1.0,
        }]
    }

    fn reset(&mut self) {
        self.last_direction = None;
    }
}
