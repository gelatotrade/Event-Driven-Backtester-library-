//! Event-driven backtesting library.
//!
//! The engine drives a chronological event loop where four kinds of events
//! flow through pluggable components:
//!
//! `MarketEvent` -> `Strategy` -> `SignalEvent` -> `Portfolio` ->
//! `OrderEvent` -> `ExecutionHandler` -> `FillEvent` -> `Portfolio`.
//!
//! This mirrors the architecture of a live trading system, so the same
//! strategy code can run against historical data and a live exchange
//! without modification.
//!
//! # Example
//!
//! ```no_run
//! use event_driven_backtester::prelude::*;
//!
//! # fn main() -> Result<(), BacktestError> {
//! let data = InMemoryDataHandler::from_bars(vec![]);
//! let strategy = MovingAverageCrossover::new("BTCUSDT", 10, 30);
//! let portfolio = Portfolio::new(100_000.0);
//! let execution = SimulatedExecutionHandler::new(
//!     LinearSlippage::new(0.0005),
//!     MakerTakerFees::new(0.0002, 0.0005),
//!     NoFunding,
//! );
//!
//! let mut engine = BacktestEngine::new(data, strategy, portfolio, execution);
//! let result = engine.run()?;
//! println!("Sharpe: {:.2}", result.metrics.sharpe);
//! # Ok(())
//! # }
//! ```

pub mod data;
pub mod engine;
pub mod error;
pub mod events;
pub mod execution;
pub mod metrics;
pub mod portfolio;
pub mod strategy;
pub mod types;
pub mod walkforward;

pub mod prelude {
    //! Common imports for users of the library.
    pub use crate::data::{Bar, CsvDataHandler, DataHandler, InMemoryDataHandler};
    pub use crate::engine::{BacktestEngine, BacktestResult};
    pub use crate::error::BacktestError;
    pub use crate::events::{
        Event, FillEvent, MarketEvent, OrderEvent, SignalDirection, SignalEvent,
    };
    pub use crate::execution::{
        ExecutionHandler, FeeModel, FixedFees, FundingModel, LinearSlippage, MakerTakerFees,
        NoFunding, PerpetualFunding, SimulatedExecutionHandler, SlippageModel, SquareRootSlippage,
        ZeroSlippage,
    };
    pub use crate::metrics::PerformanceMetrics;
    pub use crate::portfolio::{Portfolio, Position};
    pub use crate::strategy::{MovingAverageCrossover, Strategy};
    pub use crate::types::{OrderSide, OrderType, Price, Quantity, Side, Symbol, Timestamp};
    pub use crate::walkforward::{WalkForward, WalkForwardResult, WindowMode};
}
