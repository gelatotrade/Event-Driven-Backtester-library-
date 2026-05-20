//! Risk and performance metrics.
//!
//! [`PerformanceMetrics`] are derived from the equity curve (return/risk
//! statistics). [`TradeStats`] are derived from the round-trip trade log
//! (win rate, profit factor, etc.) — these are genuinely per-trade, not
//! per-bar.

use crate::portfolio::{EquityPoint, Trade};

const TRADING_DAYS_PER_YEAR: f64 = 365.0;

#[derive(Debug, Clone, Copy)]
pub struct PerformanceMetrics {
    pub total_return: f64,
    pub annualized_return: f64,
    pub annualized_volatility: f64,
    pub sharpe: f64,
    pub sortino: f64,
    pub calmar: f64,
    pub max_drawdown: f64,
    pub max_drawdown_duration: usize,
    pub num_periods: usize,
    pub final_equity: f64,
}

impl PerformanceMetrics {
    pub fn from_curve(curve: &[EquityPoint], periods_per_year: f64) -> Self {
        if curve.len() < 2 {
            return Self::empty(curve.last().map(|p| p.equity).unwrap_or(0.0));
        }

        let returns: Vec<f64> = curve
            .windows(2)
            .map(|w| {
                let prev = w[0].equity;
                let curr = w[1].equity;
                if prev.abs() < 1e-12 {
                    0.0
                } else {
                    (curr - prev) / prev
                }
            })
            .collect();

        let initial = curve.first().unwrap().equity;
        let final_eq = curve.last().unwrap().equity;
        let total_return = if initial.abs() < 1e-12 {
            0.0
        } else {
            (final_eq - initial) / initial
        };

        let mean = returns.iter().sum::<f64>() / returns.len() as f64;
        let variance =
            returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
        let std_dev = variance.sqrt();

        let downside_var = returns
            .iter()
            .filter(|r| **r < 0.0)
            .map(|r| r.powi(2))
            .sum::<f64>()
            / returns.len() as f64;
        let downside_dev = downside_var.sqrt();

        let annualized_return = mean * periods_per_year;
        let annualized_volatility = std_dev * periods_per_year.sqrt();
        let sharpe = if std_dev > 0.0 {
            (mean / std_dev) * periods_per_year.sqrt()
        } else {
            0.0
        };
        let sortino = if downside_dev > 0.0 {
            (mean / downside_dev) * periods_per_year.sqrt()
        } else {
            0.0
        };

        let (max_drawdown, max_drawdown_duration) = max_drawdown_stats(curve);
        let calmar = if max_drawdown > 0.0 {
            annualized_return / max_drawdown
        } else {
            0.0
        };

        Self {
            total_return,
            annualized_return,
            annualized_volatility,
            sharpe,
            sortino,
            calmar,
            max_drawdown,
            max_drawdown_duration,
            num_periods: returns.len(),
            final_equity: final_eq,
        }
    }

    pub fn from_daily_curve(curve: &[EquityPoint]) -> Self {
        Self::from_curve(curve, TRADING_DAYS_PER_YEAR)
    }

    fn empty(final_equity: f64) -> Self {
        Self {
            total_return: 0.0,
            annualized_return: 0.0,
            annualized_volatility: 0.0,
            sharpe: 0.0,
            sortino: 0.0,
            calmar: 0.0,
            max_drawdown: 0.0,
            max_drawdown_duration: 0,
            num_periods: 0,
            final_equity,
        }
    }
}

/// Per-trade statistics computed from the round-trip trade log.
#[derive(Debug, Clone, Copy)]
pub struct TradeStats {
    pub num_trades: usize,
    pub win_rate: f64,
    pub profit_factor: f64,
    pub avg_trade_pnl: f64,
    pub avg_win: f64,
    pub avg_loss: f64,
    pub largest_win: f64,
    pub largest_loss: f64,
    pub gross_profit: f64,
    pub gross_loss: f64,
}

impl TradeStats {
    pub fn from_trades(trades: &[Trade]) -> Self {
        if trades.is_empty() {
            return Self::empty();
        }

        let num_trades = trades.len();
        let wins: Vec<f64> = trades.iter().map(|t| t.pnl).filter(|p| *p > 0.0).collect();
        let losses: Vec<f64> = trades.iter().map(|t| t.pnl).filter(|p| *p < 0.0).collect();

        let gross_profit: f64 = wins.iter().sum();
        let gross_loss: f64 = losses.iter().map(|l| l.abs()).sum();
        let total_pnl: f64 = trades.iter().map(|t| t.pnl).sum();

        let win_rate = wins.len() as f64 / num_trades as f64;
        let profit_factor = if gross_loss > 0.0 {
            gross_profit / gross_loss
        } else if gross_profit > 0.0 {
            f64::INFINITY
        } else {
            0.0
        };
        let avg_win = if wins.is_empty() {
            0.0
        } else {
            gross_profit / wins.len() as f64
        };
        let avg_loss = if losses.is_empty() {
            0.0
        } else {
            gross_loss / losses.len() as f64
        };
        let largest_win = wins.iter().cloned().fold(0.0, f64::max);
        let largest_loss = losses.iter().cloned().fold(0.0, f64::min);

        Self {
            num_trades,
            win_rate,
            profit_factor,
            avg_trade_pnl: total_pnl / num_trades as f64,
            avg_win,
            avg_loss,
            largest_win,
            largest_loss,
            gross_profit,
            gross_loss,
        }
    }

    fn empty() -> Self {
        Self {
            num_trades: 0,
            win_rate: 0.0,
            profit_factor: 0.0,
            avg_trade_pnl: 0.0,
            avg_win: 0.0,
            avg_loss: 0.0,
            largest_win: 0.0,
            largest_loss: 0.0,
            gross_profit: 0.0,
            gross_loss: 0.0,
        }
    }
}

fn max_drawdown_stats(curve: &[EquityPoint]) -> (f64, usize) {
    let mut peak = curve.first().map(|p| p.equity).unwrap_or(0.0);
    let mut peak_idx = 0usize;
    let mut max_dd = 0.0;
    let mut max_dd_duration = 0usize;

    for (i, point) in curve.iter().enumerate() {
        if point.equity > peak {
            peak = point.equity;
            peak_idx = i;
        }
        let dd = if peak > 0.0 {
            (peak - point.equity) / peak
        } else {
            0.0
        };
        if dd > max_dd {
            max_dd = dd;
            max_dd_duration = i - peak_idx;
        }
    }

    (max_dd, max_dd_duration)
}
