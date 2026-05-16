//! Strategy abstraction: consume market events, emit trading signals.

mod ma_crossover;

pub use ma_crossover::MovingAverageCrossover;

use crate::data::DataHandler;
use crate::events::{MarketEvent, SignalEvent};

/// Strategies translate market data into trading signals.
///
/// Strategies have access to the [`DataHandler`] so they can read history
/// without buffering bars themselves.
pub trait Strategy {
    fn name(&self) -> &str;

    fn on_market(&mut self, event: &MarketEvent, data: &dyn DataHandler) -> Vec<SignalEvent>;

    fn reset(&mut self) {}
}
