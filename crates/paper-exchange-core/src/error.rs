//! Library errors: validation, order not found, insufficient balance, matching.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ExchangeError {
    #[error("validation error: {0}")]
    Validation(String),

    #[error("order not found")]
    OrderNotFound,

    #[error("insufficient balance")]
    InsufficientBalance,

    #[error("matching error: {0}")]
    Matching(String),

    #[error("invalid instrument: {0}")]
    InvalidInstrument(String),
}
