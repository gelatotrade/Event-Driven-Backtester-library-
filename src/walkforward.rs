//! Walk-forward analysis: repeatedly fit a strategy on an in-sample window
//! and evaluate it on the following out-of-sample window, rolling forward
//! through history.

use chrono::{DateTime, Duration, Utc};

use crate::data::{Bar, InMemoryDataHandler};
use crate::engine::{BacktestEngine, BacktestResult};
use crate::error::BacktestError;
use crate::execution::{FeeModel, FundingModel, SimulatedExecutionHandler, SlippageModel};
use crate::metrics::PerformanceMetrics;
use crate::portfolio::Portfolio;
use crate::strategy::Strategy;

/// Window-construction strategy for walk-forward analysis.
#[derive(Debug, Clone, Copy)]
pub enum WindowMode {
    /// Each in-sample window has the same length; the start moves forward.
    Rolling,
    /// In-sample window starts at the beginning and grows on each iteration.
    Expanding,
}

#[derive(Debug, Clone)]
pub struct WalkForwardResult {
    pub folds: Vec<WalkForwardFold>,
    pub aggregate_oos: PerformanceMetrics,
}

#[derive(Debug, Clone)]
pub struct WalkForwardFold {
    pub fold: usize,
    pub in_sample_start: DateTime<Utc>,
    pub in_sample_end: DateTime<Utc>,
    pub out_sample_start: DateTime<Utc>,
    pub out_sample_end: DateTime<Utc>,
    pub in_sample: BacktestResult,
    pub out_sample: BacktestResult,
}

/// Configuration for a walk-forward run.
pub struct WalkForward {
    in_sample_days: i64,
    out_sample_days: i64,
    mode: WindowMode,
}

impl WalkForward {
    pub fn new(in_sample_days: i64, out_sample_days: i64) -> Self {
        assert!(in_sample_days > 0 && out_sample_days > 0);
        Self {
            in_sample_days,
            out_sample_days,
            mode: WindowMode::Rolling,
        }
    }

    pub fn with_mode(mut self, mode: WindowMode) -> Self {
        self.mode = mode;
        self
    }

    /// Run walk-forward against the given full set of bars.
    ///
    /// `strategy_factory` is called fresh per fold so each window starts
    /// with a clean strategy state. Same for `portfolio_factory` and
    /// `execution_factory`.
    pub fn run<S, SL, FE, FU>(
        &self,
        bars: Vec<Bar>,
        mut strategy_factory: impl FnMut() -> S,
        mut portfolio_factory: impl FnMut() -> Portfolio,
        mut execution_factory: impl FnMut() -> SimulatedExecutionHandler<SL, FE, FU>,
    ) -> Result<WalkForwardResult, BacktestError>
    where
        S: Strategy,
        SL: SlippageModel,
        FE: FeeModel,
        FU: FundingModel,
    {
        if bars.is_empty() {
            return Err(BacktestError::Data(
                "walk-forward requires non-empty bars".into(),
            ));
        }

        let mut sorted = bars;
        sorted.sort_by_key(|b| b.timestamp);
        let global_start = sorted.first().unwrap().timestamp;
        let global_end = sorted.last().unwrap().timestamp;

        let is_dur = Duration::days(self.in_sample_days);
        let oos_dur = Duration::days(self.out_sample_days);

        let mut folds = Vec::new();
        let mut iter_is_start = global_start;
        let mut fold_index = 0usize;

        loop {
            let is_start = match self.mode {
                WindowMode::Rolling => iter_is_start,
                WindowMode::Expanding => global_start,
            };
            let is_end = iter_is_start + is_dur;
            let oos_start = is_end;
            let oos_end = oos_start + oos_dur;

            if oos_end > global_end + Duration::days(1) {
                break;
            }

            let is_bars: Vec<Bar> = sorted
                .iter()
                .filter(|b| b.timestamp >= is_start && b.timestamp < is_end)
                .cloned()
                .collect();
            let oos_bars: Vec<Bar> = sorted
                .iter()
                .filter(|b| b.timestamp >= oos_start && b.timestamp < oos_end)
                .cloned()
                .collect();

            if is_bars.is_empty() || oos_bars.is_empty() {
                iter_is_start += oos_dur;
                continue;
            }

            let is_result = run_segment(
                is_bars,
                strategy_factory(),
                portfolio_factory(),
                execution_factory(),
            )?;
            let oos_result = run_segment(
                oos_bars,
                strategy_factory(),
                portfolio_factory(),
                execution_factory(),
            )?;

            folds.push(WalkForwardFold {
                fold: fold_index,
                in_sample_start: is_start,
                in_sample_end: is_end,
                out_sample_start: oos_start,
                out_sample_end: oos_end,
                in_sample: is_result,
                out_sample: oos_result,
            });

            fold_index += 1;
            iter_is_start += oos_dur;
        }

        // Aggregate OOS metrics by stitching equity curves together as percent returns.
        let aggregate_curve = stitch_curves(folds.iter().map(|f| &f.out_sample));
        let aggregate_oos = PerformanceMetrics::from_curve(&aggregate_curve, 365.0);

        Ok(WalkForwardResult {
            folds,
            aggregate_oos,
        })
    }
}

fn run_segment<S, SL, FE, FU>(
    bars: Vec<Bar>,
    strategy: S,
    portfolio: Portfolio,
    execution: SimulatedExecutionHandler<SL, FE, FU>,
) -> Result<BacktestResult, BacktestError>
where
    S: Strategy,
    SL: SlippageModel,
    FE: FeeModel,
    FU: FundingModel,
{
    let data = InMemoryDataHandler::from_bars(bars);
    let mut engine = BacktestEngine::new(data, strategy, portfolio, execution);
    engine.run()
}

fn stitch_curves<'a>(
    results: impl Iterator<Item = &'a BacktestResult>,
) -> Vec<crate::portfolio::EquityPoint> {
    let mut stitched: Vec<crate::portfolio::EquityPoint> = Vec::new();
    let mut scale = 1.0;

    for result in results {
        if result.equity_curve.is_empty() {
            continue;
        }
        let start = result.equity_curve.first().unwrap().equity;
        if start.abs() < 1e-12 {
            continue;
        }
        for point in &result.equity_curve {
            let normalized = scale * (point.equity / start);
            stitched.push(crate::portfolio::EquityPoint {
                timestamp: point.timestamp,
                equity: normalized,
                cash: point.cash,
                position_value: point.position_value,
            });
        }
        let last = result.equity_curve.last().unwrap();
        scale *= last.equity / start;
    }
    stitched
}
