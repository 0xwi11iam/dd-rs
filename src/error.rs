/// dd-rs error types.
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Invalid size suffix in '{input}': {reason}")]
    InvalidSize { input: String, reason: String },

    #[error("Output file already exists (conv=excl): {path}")]
    FileExists { path: PathBuf },

    #[error("Output file does not exist (conv=nocreat): {path}")]
    FileNotFound { path: PathBuf },

    #[error("Read error at offset {offset}: {source}")]
    ReadError {
        offset: u64,
        #[source]
        source: std::io::Error,
    },

    #[error("Write error at offset {offset}: {source}")]
    WriteError {
        offset: u64,
        #[source]
        source: std::io::Error,
    },

    #[error("Conversion error: {0}")]
    Conversion(String),

    #[error("Invalid flag: {flag}={value}")]
    InvalidFlag { flag: String, value: String },

    #[error("{0}")]
    Other(String),
}

/// Result type alias used throughout dd-rs.
pub type Result<T> = std::result::Result<T, Error>;
