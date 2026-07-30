/// Signal handling: SIGUSR1 triggers an immediate status dump.
///
/// When the dd-rs process receives SIGUSR1, it prints current I/O statistics
/// to stderr, matching GNU dd behaviour:
///   "XXX+NN records in, YYY+MM records out"
///
/// We use `signal-hook` to register a thread-safe handler that sets an atomic
/// flag. The main I/O loop polls this flag periodically.

use std::sync::atomic::{AtomicBool, Ordering};

/// Global flag: set to true when SIGUSR1 is received.
static SIGUSR1_RECEIVED: AtomicBool = AtomicBool::new(false);

/// Register the SIGUSR1 handler. Should be called once at startup.
pub fn install_signal_handlers() -> Result<(), crate::error::Error> {
    // Register SIGUSR1 — sets the global atomic flag.
    // SAFETY: signal_hook::low_level::register is safe to call. The handler
    // only sets an atomic bool, which is signal-safe.
    unsafe {
        signal_hook::low_level::register(signal_hook::consts::SIGUSR1, || {
            SIGUSR1_RECEIVED.store(true, Ordering::SeqCst);
        })
        .map_err(|e| crate::error::Error::Other(format!(
            "Failed to register SIGUSR1 handler: {}",
            e
        )))?;

        // SIGUSR2 also triggers a stats dump
        signal_hook::low_level::register(signal_hook::consts::SIGUSR2, || {
            SIGUSR1_RECEIVED.store(true, Ordering::SeqCst);
        })
        .map_err(|e| crate::error::Error::Other(format!(
            "Failed to register SIGUSR2 handler: {}",
            e
        )))?;
    }

    Ok(())
}

/// Check if SIGUSR1 has been received and reset the flag.
pub fn check_signal() -> bool {
    SIGUSR1_RECEIVED.swap(false, Ordering::SeqCst)
}

/// Print current stats (signal-triggered).
pub fn print_signal_stats(
    full_in: u64,
    partial_in: u64,
    full_out: u64,
    partial_out: u64,
    bytes: u64,
) {
    eprintln!(
        "{}+{} records in\n{}+{} records out\n{} bytes\n",
        full_in, partial_in, full_out, partial_out, bytes
    );
}
