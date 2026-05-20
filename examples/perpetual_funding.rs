//! Backtest a perpetual-futures strategy that pays/receives funding every 8h.
//!
//! ```bash
//! cargo run --release --example perpetual_funding
//! ```

use chrono::{Duration, TimeZone, Utc};
use event_driven_backtester::prelude::*;

fn main() -> Result<(), BacktestError> {
    let bars = perpetual_bars("ETHPERP", 24 * 60);
    let strategy = MovingAverageCrossover::new("ETHPERP", 20, 60);
    let portfolio = Portfolio::new(100_000.0)
        .with_risk_per_trade(0.75)
        .with_max_leverage(3.0)
        .with_maintenance_margin(0.005) // liquidate below 0.5% maintenance margin
        .with_annual_borrow_rate(0.10) // 10% p.a. financing on shorts
        .with_liquidation_fee(0.005); // 50 bps penalty if liquidated
    let execution = SimulatedExecutionHandler::new(
        SquareRootSlippage::new(0.5, 0.0001),
        MakerTakerFees::new(0.0001, 0.0004),
        PerpetualFunding,
    );

    let mut engine = BacktestEngine::new(
        InMemoryDataHandler::from_bars(bars),
        strategy,
        portfolio,
        execution,
    )
    .with_periods_per_year(365.0 * 24.0); // hourly bars

    let result = engine.run()?;

    println!("ETHPERP funding-aware backtest");
    println!("------------------------------");
    println!("Final equity      : {:>12.2}", result.metrics.final_equity);
    println!("Sharpe            : {:>12.2}", result.metrics.sharpe);
    println!(
        "Max drawdown      : {:>12.2}%",
        result.metrics.max_drawdown * 100.0
    );
    println!("Total funding paid: {:>12.2}", result.total_funding);
    println!("Total borrow cost : {:>12.2}", result.total_borrow);
    println!("Round-trip trades : {:>12}", result.trade_stats.num_trades);
    println!("Liquidated        : {:>12}", result.liquidated);
    Ok(())
}

fn perpetual_bars(symbol: &str, hours: usize) -> Vec<Bar> {
    let start = Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap();
    let mut bars = Vec::with_capacity(hours);
    let mut price = 3_500.0_f64;
    for i in 0..hours {
        let phase = (i as f64) * 0.02;
        let drift = phase.sin() * 200.0;
        let noise = ((i * 17) % 13) as f64 - 6.0;
        let close = 3_500.0 + drift + noise * 1.5;
        let open = price;
        let high = open.max(close) + 5.0;
        let low = open.min(close) - 5.0;

        // Funding settles every 8h (= every 8th bar). Synthetic skew toward longs paying.
        let funding_rate = if i % 8 == 0 && i > 0 {
            Some(0.00005 + 0.00003 * phase.sin())
        } else {
            None
        };

        let mut bar = Bar::new(
            start + Duration::hours(i as i64),
            symbol,
            open,
            high,
            low,
            close,
            5_000.0,
        );
        bar.funding_rate = funding_rate;
        bars.push(bar);
        price = close;
    }
    bars
}
