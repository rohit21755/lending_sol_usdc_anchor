use anchor_lang::prelude::*;

#[error_code]
pub enum ErrorCode {
    #[msg("Insuficient Funds")]
    InsufficientFunds,
    #[msg("Over Borrowable Amount")]
    OverBorrowable,
    #[msg("Over Repay Amount")]
    OverRepay,
}