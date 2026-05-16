//! Risk and performance metrics computed from an equity curve.

use crate::portfolio::EquityPoint;

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
    pub profit_factor: f64,
    pub win_rate: f64,
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
        let variance = returns
            .iter()
            .map(|r| (r - mean).powi(2))
            .sum::<f64>()
            / returns.len() as f64;
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

        let gains: f64 = returns.iter().filter(|r| **r > 0.0).sum();
        let losses: f64 = returns.iter().filter(|r| **r < 0.0).map(|r| r.abs()).sum();
        let profit_factor = if losses > 0.0 {
            gains / losses
        } else if gains > 0.0 {
            f64::INFINITY
        } else {
            0.0
        };

        let wins = returns.iter().filter(|r| **r > 0.0).count();
        let non_zero = returns.iter().filter(|r| r.abs() > 1e-12).count();
        let win_rate = if non_zero > 0 {
            wins as f64 / non_zero as f64
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
            profit_factor,
            win_rate,
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
            profit_factor: 0.0,
            win_rate: 0.0,
            num_periods: 0,
            final_equity,
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
