//! Walk-forward analysis demo (rolling 90d in-sample / 30d out-of-sample).
//!
//! ```bash
//! cargo run --release --example walk_forward
//! ```

use chrono::{Duration, TimeZone, Utc};
use event_driven_backtester::prelude::*;

fn main() -> Result<(), BacktestError> {
    let bars = synthetic_bars("BTCUSDT", 720);

    let wf = WalkForward::new(90, 60).with_mode(WindowMode::Rolling);

    let result = wf.run(
        bars,
        || MovingAverageCrossover::new("BTCUSDT", 5, 15),
        || Portfolio::new(100_000.0).with_risk_per_trade(0.5),
        || {
            SimulatedExecutionHandler::new(
                LinearSlippage::new(0.0005),
                MakerTakerFees::new(0.0002, 0.0005),
                NoFunding,
            )
        },
    )?;

    println!("Walk-forward analysis");
    println!("---------------------");
    println!(
        "{:>5} {:>11} {:>11} {:>10} {:>10}",
        "fold", "is_start", "oos_start", "is_sharpe", "oos_sharpe"
    );
    for fold in &result.folds {
        println!(
            "{:>5} {:>11} {:>11} {:>10.2} {:>10.2}",
            fold.fold,
            fold.in_sample_start.date_naive(),
            fold.out_sample_start.date_naive(),
            fold.in_sample.metrics.sharpe,
            fold.out_sample.metrics.sharpe,
        );
    }

    println!();
    println!("Stitched OOS Sharpe : {:.2}", result.aggregate_oos.sharpe);
    println!(
        "Stitched OOS return : {:.2}%",
        result.aggregate_oos.total_return * 100.0
    );
    println!(
        "Stitched OOS max DD : {:.2}%",
        result.aggregate_oos.max_drawdown * 100.0
    );
    Ok(())
}

fn synthetic_bars(symbol: &str, n: usize) -> Vec<Bar> {
    let start = Utc.with_ymd_and_hms(2023, 1, 1, 0, 0, 0).unwrap();
    let mut bars = Vec::with_capacity(n);
    let mut price: f64 = 30_000.0;
    for i in 0..n {
        let phase = (i as f64) * 0.04;
        let drift = phase.sin() * 2_500.0;
        let trend = (i as f64) * 5.0;
        let noise = ((i * 41) % 17) as f64 - 8.0;
        let close = 30_000.0 + drift + trend + noise * 30.0;
        let open = price;
        let high = open.max(close) + 70.0;
        let low = open.min(close) - 70.0;
        bars.push(Bar::new(
            start + Duration::days(i as i64),
            symbol,
            open,
            high,
            low,
            close,
            1_500.0,
        ));
        price = close;
    }
    bars
}
