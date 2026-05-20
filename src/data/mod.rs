//! Market-data sources.
//!
//! A [`DataHandler`] streams bars chronologically to the engine and keeps
//! a rolling history available for strategies to inspect.

mod bar;
mod csv;
mod memory;

pub use bar::Bar;
pub use csv::CsvDataHandler;
pub use memory::InMemoryDataHandler;

use crate::events::MarketEvent;
use crate::types::Symbol;

pub trait DataHandler {
    /// Advance to the next bar; returns `None` when exhausted.
    fn next(&mut self) -> Option<MarketEvent>;

    /// Most recent bar emitted for `symbol`, if any.
    fn current_bar(&self, symbol: &str) -> Option<&Bar>;

    /// Last `n` bars for `symbol`, oldest first. Returns fewer if history is short.
    fn history(&self, symbol: &str, n: usize) -> Vec<Bar>;

    /// All symbols the handler may emit.
    fn symbols(&self) -> Vec<Symbol>;

    /// Reset to the beginning of the data stream.
    fn reset(&mut self);
}
