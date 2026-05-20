use thiserror::Error;

#[derive(Debug, Error)]
pub enum BacktestError {
    #[error("data error: {0}")]
    Data(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("csv error: {0}")]
    Csv(#[from] csv::Error),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("invalid order: {0}")]
    InvalidOrder(String),

    #[error("insufficient cash: required {required:.2}, have {available:.2}")]
    InsufficientCash { required: f64, available: f64 },

    #[error("unknown symbol: {0}")]
    UnknownSymbol(String),

    #[error("configuration error: {0}")]
    Config(String),
}
