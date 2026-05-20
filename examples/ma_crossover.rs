//! Run a simple moving-average crossover on synthetic daily bars.
//!
//! ```bash
//! cargo run --release --example ma_crossover
//! ```

use chrono::{Duration, TimeZone, Utc};
use event_driven_backtester::prelude::*;

fn main() -> Result<(), BacktestError> {
    let bars = synthetic_bars("BTCUSDT", 365);

    let data = InMemoryDataHandler::from_bars(bars);
    let strategy = MovingAverageCrossover::new("BTCUSDT", 10, 30);
    let portfolio = Portfolio::new(100_000.0)
        .with_risk_per_trade(0.5)
        .with_max_leverage(1.0);
    let execution = SimulatedExecutionHandler::new(
        LinearSlippage::new(0.0005),       // 5 bps
        MakerTakerFees::new(0.0002, 0.0005), // 2 / 5 bps
        NoFunding,
    );

    let mut engine = BacktestEngine::new(data, strategy, portfolio, execution);
    let result = engine.run()?;

    println!("MA crossover backtest");
    println!("---------------------");
    println!("Final equity      : {:>12.2}", result.metrics.final_equity);
    println!("Total return      : {:>12.2}%", result.metrics.total_return * 100.0);
    println!("Annualized return : {:>12.2}%", result.metrics.annualized_return * 100.0);
    println!("Annualized vol    : {:>12.2}%", result.metrics.annualized_volatility * 100.0);
    println!("Sharpe            : {:>12.2}", result.metrics.sharpe);
    println!("Sortino           : {:>12.2}", result.metrics.sortino);
    println!("Calmar            : {:>12.2}", result.metrics.calmar);
    println!("Max drawdown      : {:>12.2}%", result.metrics.max_drawdown * 100.0);
    println!("Profit factor     : {:>12.2}", result.metrics.profit_factor);
    println!("Win rate          : {:>12.2}%", result.metrics.win_rate * 100.0);
    println!("Trades            : {:>12}", result.trade_count);
    println!("Total fees        : {:>12.2}", result.total_fees);
    println!("Total slippage    : {:>12.2}", result.total_slippage);
    Ok(())
}

/// Generates a sinusoidal price series with light noise so the crossover
/// strategy has actual cycles to trade. Reproducible — no RNG.
fn synthetic_bars(symbol: &str, n: usize) -> Vec<Bar> {
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let mut bars = Vec::with_capacity(n);
    let mut price: f64 = 30_000.0;
    for i in 0..n {
        let phase = (i as f64) * 0.05;
        let drift = phase.sin() * 1500.0;
        let noise = ((i * 37) % 11) as f64 - 5.0;
        let close = 30_000.0 + drift + noise * 25.0;
        let open = price;
        let high = open.max(close) + 60.0;
        let low = open.min(close) - 60.0;
        bars.push(Bar::new(
            start + Duration::days(i as i64),
            symbol,
            open,
            high,
            low,
            close,
            1_000.0,
        ));
        price = close;
    }
    bars
}
