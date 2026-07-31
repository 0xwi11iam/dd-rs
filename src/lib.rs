//! dd-rs library — a safe, modern Rust+C alternative to dd.
//!
//! ## Module overview
//!
//! | Module       | Purpose                                         |
//! |------------- |-------------------------------------------------|
//! | `args`       | CLI argument parsing (clap)                     |
//! | `sizes`      | Size suffix parsing (4K, 10M, 4xM, 512B, etc.) |
//! | `conv`       | Conversion pipeline (ebcdic, block, swab, etc.) |
//! | `conv::block`| block/unblock record conversions                |
//! | `conv::case` | lcase/ucase character conversions               |
//! | `conv::swab` | Byte-pair swapping                              |
//! | `conv::ebcdic`| FFI to C EBCDIC tables                         |
//! | `flags`      | I/O flags (iflag=/oflag=) and file opening      |
//! | `io_engine`  | Core read→convert→write loop                    |
//! | `signal`     | SIGUSR1 handling for progress dumps             |
//! | `status`     | Progress reporting and final statistics         |
//! | `error`      | Error types                                     |
//!
//! ## Usage
//!
//! ```no_run
//! use dd_rs::args::{CliArgs, resolve_config};
//! use clap::Parser;
//!
//! // Parse CLI
//! let args = CliArgs::parse_from(["dd-rs", "if=input.dat", "of=output.dat", "bs=1M"]);
//! let config = resolve_config(args, false).unwrap();
//!
//! // Or use programmatically
//! use dd_rs::io_engine::{EngineConfig, run_transfer};
//! // ...
//! ```

pub mod args;
pub mod conv;
pub mod error;
pub mod explain;
pub mod flags;
pub mod io_engine;
pub mod safety;
pub mod signal;
pub mod sizes;
pub mod status;

// Re-export commonly used types
pub use error::{Error, Result};
