use chrono::{Duration, TimeZone, Utc};
use event_driven_backtester::prelude::*;

fn ramp_bars(symbol: &str, n: usize, start_price: f64, step: f64) -> Vec<Bar> {
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    (0..n)
        .map(|i| {
            let close = start_price + step * (i as f64);
            let open = if i == 0 { close } else { start_price + step * ((i - 1) as f64) };
            Bar::new(
                start + Duration::days(i as i64),
                symbol,
                open,
                close.max(open) + 1.0,
                close.min(open) - 1.0,
                close,
                1_000.0,
            )
        })
        .collect()
}

#[test]
fn ma_crossover_runs_end_to_end() {
    let bars = ramp_bars("BTC", 100, 100.0, 1.0);
    let data = InMemoryDataHandler::from_bars(bars);
    let strategy = MovingAverageCrossover::new("BTC", 5, 20);
    let portfolio = Portfolio::new(10_000.0);
    let execution = SimulatedExecutionHandler::new(
        ZeroSlippage,
        FixedFees::new(0.0),
        NoFunding,
    );

    let mut engine = BacktestEngine::new(data, strategy, portfolio, execution);
    let result = engine.run().expect("engine should run");

    assert!(!result.equity_curve.is_empty());
    assert!(result.trade_count >= 1, "should have opened at least one position on a rising ramp");
    assert!(result.metrics.num_periods > 0);
}

#[test]
fn linear_slippage_moves_buy_price_up_and_sell_price_down() {
    let bar = Bar::new(
        Utc::now(),
        "BTC",
        100.0, 110.0, 95.0, 105.0,
        1_000.0,
    );

    let order_buy = OrderEvent {
        timestamp: bar.timestamp,
        symbol: "BTC".into(),
        side: OrderSide::Buy,
        quantity: 1.0,
        order_type: OrderType::Market,
        limit_price: None,
    };

    let order_sell = OrderEvent {
        timestamp: bar.timestamp,
        symbol: "BTC".into(),
        side: OrderSide::Sell,
        quantity: 1.0,
        order_type: OrderType::Market,
        limit_price: None,
    };

    let mut exec = SimulatedExecutionHandler::new(
        LinearSlippage::new(0.01),
        FixedFees::new(0.0),
        NoFunding,
    );

    let buy_fill = exec.execute(&order_buy, &bar).unwrap();
    let sell_fill = exec.execute(&order_sell, &bar).unwrap();

    // 1% of close = 1.05, applied around bar.open = 100.0
    approx::assert_relative_eq!(buy_fill.fill_price, 101.05, max_relative = 1e-9);
    approx::assert_relative_eq!(sell_fill.fill_price, 98.95, max_relative = 1e-9);
    assert!(!buy_fill.is_maker);
}

#[test]
fn maker_taker_fees_charge_correct_rate() {
    let bar = Bar::new(
        Utc::now(),
        "BTC",
        100.0, 110.0, 95.0, 105.0,
        1_000.0,
    );
    let order = OrderEvent {
        timestamp: bar.timestamp,
        symbol: "BTC".into(),
        side: OrderSide::Buy,
        quantity: 2.0,
        order_type: OrderType::Limit,
        limit_price: Some(96.0),
    };

    let mut exec = SimulatedExecutionHandler::new(
        ZeroSlippage,
        MakerTakerFees::new(0.0001, 0.001),
        NoFunding,
    );

    let fill = exec.execute(&order, &bar).unwrap();
    // Limit fills are maker -> notional * 0.0001 = 96 * 2 * 0.0001 = 0.0192
    approx::assert_relative_eq!(fill.commission, 0.0192, max_relative = 1e-9);
    assert!(fill.is_maker);
}

#[test]
fn funding_payments_reduce_long_cash() {
    let mut portfolio = Portfolio::new(10_000.0);

    let buy = FillEvent {
        timestamp: Utc::now(),
        symbol: "ETH".into(),
        side: OrderSide::Buy,
        quantity: 10.0,
        fill_price: 1_000.0,
        commission: 0.0,
        slippage_cost: 0.0,
        is_maker: false,
    };
    portfolio.on_fill(&buy);
    portfolio.mark_to_market(Utc::now(), "ETH", 1_000.0);

    let cash_before = portfolio.cash();
    portfolio.apply_funding("ETH", 0.0001); // 1 bps
    let cash_after = portfolio.cash();

    // Long pays positive funding: notional * rate = 10 * 1000 * 0.0001 = 1.0
    approx::assert_relative_eq!(cash_before - cash_after, 1.0, max_relative = 1e-9);
}

#[test]
fn portfolio_tracks_realized_pnl_through_round_trip() {
    let mut portfolio = Portfolio::new(10_000.0);
    let ts = Utc::now();

    portfolio.on_fill(&FillEvent {
        timestamp: ts,
        symbol: "BTC".into(),
        side: OrderSide::Buy,
        quantity: 1.0,
        fill_price: 100.0,
        commission: 0.0,
        slippage_cost: 0.0,
        is_maker: false,
    });
    portfolio.on_fill(&FillEvent {
        timestamp: ts,
        symbol: "BTC".into(),
        side: OrderSide::Sell,
        quantity: 1.0,
        fill_price: 120.0,
        commission: 0.0,
        slippage_cost: 0.0,
        is_maker: false,
    });

    approx::assert_relative_eq!(portfolio.realized_pnl(), 20.0, max_relative = 1e-9);
    assert!(portfolio.position("BTC").map(|p| p.is_flat()).unwrap_or(false));
}

#[test]
fn walk_forward_produces_folds() {
    let bars = ramp_bars("BTC", 360, 100.0, 0.5);

    let wf = WalkForward::new(60, 30);
    let result = wf
        .run(
            bars,
            || MovingAverageCrossover::new("BTC", 5, 20),
            || Portfolio::new(10_000.0),
            || SimulatedExecutionHandler::new(ZeroSlippage, FixedFees::new(0.0), NoFunding),
        )
        .expect("walk-forward should run");

    assert!(result.folds.len() >= 5, "expected several folds, got {}", result.folds.len());
    for fold in &result.folds {
        assert!(fold.in_sample_end == fold.out_sample_start);
    }
}
