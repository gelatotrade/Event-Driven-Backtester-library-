//! Main backtest engine — drives the event loop.

use std::collections::VecDeque;

use crate::data::DataHandler;
use crate::error::BacktestError;
use crate::events::{Event, MarketEvent};
use crate::execution::{
    ExecutionHandler, FeeModel, FundingModel, SimulatedExecutionHandler, SlippageModel,
};
use crate::metrics::PerformanceMetrics;
use crate::portfolio::{EquityPoint, Portfolio};
use crate::strategy::Strategy;

/// Output of a backtest run.
#[derive(Debug, Clone)]
pub struct BacktestResult {
    pub metrics: PerformanceMetrics,
    pub equity_curve: Vec<EquityPoint>,
    pub realized_pnl: f64,
    pub total_fees: f64,
    pub total_funding: f64,
    pub total_slippage: f64,
    pub trade_count: usize,
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
    pub fn run(&mut self) -> Result<BacktestResult, BacktestError> {
        let mut queue: VecDeque<Event> = VecDeque::new();

        while let Some(market) = self.data.next() {
            self.handle_market(&market);
            queue.push_back(Event::Market(market));

            while let Some(event) = queue.pop_front() {
                match event {
                    Event::Market(m) => {
                        let signals = self.strategy.on_market(&m, &self.data);
                        for sig in signals {
                            queue.push_back(Event::Signal(sig));
                        }
                    }
                    Event::Signal(sig) => {
                        if let Some(order) = self.portfolio.on_signal(&sig, &self.data)? {
                            queue.push_back(Event::Order(order));
                        }
                    }
                    Event::Order(order) => {
                        let bar = self
                            .data
                            .current_bar(&order.symbol)
                            .ok_or_else(|| BacktestError::UnknownSymbol(order.symbol.clone()))?
                            .clone();
                        match self.execution.execute(&order, &bar) {
                            Ok(fill) => queue.push_back(Event::Fill(fill)),
                            Err(BacktestError::InvalidOrder(_)) => continue,
                            Err(e) => return Err(e),
                        }
                    }
                    Event::Fill(fill) => {
                        self.portfolio.on_fill(&fill);
                    }
                }
            }
        }

        let curve = self.portfolio.equity_curve().to_vec();
        let metrics = PerformanceMetrics::from_curve(&curve, self.periods_per_year);

        Ok(BacktestResult {
            metrics,
            equity_curve: curve,
            realized_pnl: self.portfolio.realized_pnl(),
            total_fees: self.portfolio.total_fees(),
            total_funding: self.portfolio.total_funding(),
            total_slippage: self.portfolio.total_slippage(),
            trade_count: self.portfolio.trade_count(),
        })
    }

    fn handle_market(&mut self, market: &MarketEvent) {
        self.portfolio
            .mark_to_market(market.timestamp, &market.bar.symbol, market.bar.close);

        if let Some(rate) = self.execution.funding_for(&market.bar) {
            self.portfolio.apply_funding(&market.bar.symbol, rate);
        }
    }
}
