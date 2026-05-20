//! Main backtest engine — drives the event loop.

use std::path::Path;

use crate::data::DataHandler;
use crate::error::BacktestError;
use crate::events::{MarketEvent, OrderEvent};
use crate::execution::{
    ExecutionHandler, FeeModel, FundingModel, SimulatedExecutionHandler, SlippageModel,
};
use crate::metrics::{PerformanceMetrics, TradeStats};
use crate::portfolio::{EquityPoint, Portfolio, Trade};
use crate::strategy::Strategy;

/// Output of a backtest run.
#[derive(Debug, Clone)]
pub struct BacktestResult {
    pub metrics: PerformanceMetrics,
    pub trade_stats: TradeStats,
    pub equity_curve: Vec<EquityPoint>,
    pub trades: Vec<Trade>,
    pub realized_pnl: f64,
    pub total_fees: f64,
    pub total_funding: f64,
    pub total_borrow: f64,
    pub total_slippage: f64,
    /// Number of executed fills.
    pub fills: usize,
    /// True if the account was force-liquidated during the run.
    pub liquidated: bool,
}

impl BacktestResult {
    /// Write the round-trip trade log to a CSV file.
    pub fn write_trades_csv(&self, path: impl AsRef<Path>) -> Result<(), BacktestError> {
        let mut wtr = csv::Writer::from_path(path)?;
        for trade in &self.trades {
            wtr.serialize(trade)?;
        }
        wtr.flush()?;
        Ok(())
    }

    /// Write the equity curve to a CSV file.
    pub fn write_equity_csv(&self, path: impl AsRef<Path>) -> Result<(), BacktestError> {
        let mut wtr = csv::Writer::from_path(path)?;
        for point in &self.equity_curve {
            wtr.serialize(point)?;
        }
        wtr.flush()?;
        Ok(())
    }
}

pub struct BacktestEngine<D, S, SL, FE, FU>
where
    D: DataHandler,
    S: Strategy,
    SL: SlippageModel,
    FE: FeeModel,
    FU: FundingModel,
{
    data: D,
    strategy: S,
    portfolio: Portfolio,
    execution: SimulatedExecutionHandler<SL, FE, FU>,
    periods_per_year: f64,
}

impl<D, S, SL, FE, FU> BacktestEngine<D, S, SL, FE, FU>
where
    D: DataHandler,
    S: Strategy,
    SL: SlippageModel,
    FE: FeeModel,
    FU: FundingModel,
{
    pub fn new(
        data: D,
        strategy: S,
        portfolio: Portfolio,
        execution: SimulatedExecutionHandler<SL, FE, FU>,
    ) -> Self {
        Self {
            data,
            strategy,
            portfolio,
            execution,
            periods_per_year: 365.0,
        }
    }

    /// Override the annualization factor (default = 365 daily bars per year).
    pub fn with_periods_per_year(mut self, periods: f64) -> Self {
        assert!(periods > 0.0);
        self.periods_per_year = periods;
        self
    }

    pub fn portfolio(&self) -> &Portfolio {
        &self.portfolio
    }

    /// Execute the backtest end-to-end.
    ///
    /// Orders are filled on the **next** bar of their symbol, at that bar's
    /// open, so a signal computed from bar T's close never executes at a
    /// price from bar T or earlier — this is what keeps the simulation free
    /// of look-ahead bias.
    pub fn run(&mut self) -> Result<BacktestResult, BacktestError> {
        let mut pending: Vec<OrderEvent> = Vec::new();

        while let Some(market) = self.data.next() {
            let sym = market.bar.symbol.clone();

            // Phase 1: fill orders queued on the previous bar of this symbol,
            // at the current bar's open.
            let mut i = 0;
            while i < pending.len() {
                if pending[i].symbol == sym {
                    let order = pending.remove(i);
                    match self.execution.execute(&order, &market.bar) {
                        Ok(fill) => self.portfolio.on_fill(&fill),
                        // A limit order that wasn't triggered this bar expires.
                        Err(BacktestError::InvalidOrder(_)) => {}
                        Err(e) => return Err(e),
                    }
                } else {
                    i += 1;
                }
            }

            // Phase 2: funding + borrow + mark-to-market at the bar close.
            self.handle_market(&market);

            // Phase 3: liquidation check after marking to market.
            self.portfolio.check_and_liquidate(market.timestamp);

            // Phase 4: snapshot equity for this bar (post-fill, post-liquidation).
            self.portfolio.snapshot_equity(market.timestamp);

            // Phase 5: strategy generates signals -> orders queued for the next bar.
            if !self.portfolio.is_liquidated() {
                let signals = self.strategy.on_market(&market, &self.data);
                for sig in signals {
                    if let Some(order) = self.portfolio.on_signal(&sig, &self.data)? {
                        pending.push(order);
                    }
                }
            }
        }

        let curve = self.portfolio.equity_curve().to_vec();
        let trades = self.portfolio.trades().to_vec();
        let metrics = PerformanceMetrics::from_curve(&curve, self.periods_per_year);
        let trade_stats = TradeStats::from_trades(&trades);

        Ok(BacktestResult {
            metrics,
            trade_stats,
            equity_curve: curve,
            trades,
            realized_pnl: self.portfolio.realized_pnl(),
            total_fees: self.portfolio.total_fees(),
            total_funding: self.portfolio.total_funding(),
            total_borrow: self.portfolio.total_borrow(),
            total_slippage: self.portfolio.total_slippage(),
            fills: self.portfolio.fills(),
            liquidated: self.portfolio.is_liquidated(),
        })
    }

    fn handle_market(&mut self, market: &MarketEvent) {
        self.portfolio
            .update_mark(&market.bar.symbol, market.bar.close);

        if let Some(rate) = self.execution.funding_for(&market.bar) {
            self.portfolio.apply_funding(&market.bar.symbol, rate);
        }
        self.portfolio.apply_borrow_cost(self.periods_per_year);
    }
}
