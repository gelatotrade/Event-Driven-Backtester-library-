//! Order execution simulation.
//!
//! [`ExecutionHandler`] turns an [`OrderEvent`] into a [`FillEvent`] using
//! pluggable [`SlippageModel`], [`FeeModel`] and [`FundingModel`].

mod fees;
mod funding;
mod simulated;
mod slippage;

pub use fees::{FeeModel, FixedFees, MakerTakerFees};
pub use funding::{FundingModel, NoFunding, PerpetualFunding};
pub use simulated::SimulatedExecutionHandler;
pub use slippage::{LinearSlippage, SlippageModel, SquareRootSlippage, ZeroSlippage};

use crate::data::Bar;
use crate::error::BacktestError;
use crate::events::{FillEvent, OrderEvent};

pub trait ExecutionHandler {
    /// Simulate execution of `order` given the current market bar.
    fn execute(&mut self, order: &OrderEvent, bar: &Bar) -> Result<FillEvent, BacktestError>;
}
