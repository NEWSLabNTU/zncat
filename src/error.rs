use std::io;

/// The type alias replacing the error type to [Error](Error).
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// The universal error type unsed across the crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Error from Zenoh: {0}")]
    Zenoh(#[from] zenoh::Error),

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("JSON serialization error: {0}")]
    JsonSerialization(#[from] serde_json::Error),

    #[error("--lb and --block-size cannot be specified simultaneously")]
    InvalidBufferingOptions,

    #[error("At least one of --pub or --sub must be specified")]
    NoPubSubOptions,
}

pub fn ok<T>(value: T) -> Result<T> {
    Ok(value)
}
