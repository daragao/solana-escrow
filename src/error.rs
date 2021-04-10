use solana_program::program_error::ProgramError;
use thiserror::Error;

#[derive(Debug, Error, Copy, Clone)]
pub enum EscrowError {
    #[error("Invalid Instruction")]
    InvalidInstruction,
    #[error("Not Rent Exempt")]
    NotRentExempt,
    #[error("Expected amount mismatch")]
    ExpectedAmountMismatch,
    #[error("Amount overflow")]
    AmountOverflow,
}

impl From<EscrowError> for ProgramError {
    fn from(e: EscrowError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
