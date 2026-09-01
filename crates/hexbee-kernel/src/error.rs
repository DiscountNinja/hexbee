use thiserror::Error;

use hexbee_store::StoreError;

#[derive(Debug, Error)]
pub enum KernelError {
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("no active hex session")]
    NoActiveHex,
    #[error("capability denied: {0}")]
    CapabilityDenied(String),
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("hex: {0}")]
    Hex(String),
}

pub type Result<T> = std::result::Result<T, KernelError>;
