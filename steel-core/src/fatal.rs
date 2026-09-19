//! Unrecoverable runtime failures.
//!
//! Storage layers sit far from the server's lifecycle, so the server registers
//! its shutdown token here and they request a stop through it.

use std::sync::OnceLock;

use tokio_util::sync::CancellationToken;

static FATAL_SHUTDOWN: OnceLock<CancellationToken> = OnceLock::new();

/// Registers the token that [`request_fatal_shutdown`] cancels. Called once
/// during startup; later calls are ignored.
pub fn register_fatal_shutdown(token: CancellationToken) {
    let _ = FATAL_SHUTDOWN.set(token);
}

/// Stops the server because it can no longer persist anything.
///
/// Continuing would silently discard everything players do from here on.
/// Shutdown is requested rather than forced so teardown still runs, though it
/// is best-effort: the failure that triggered this usually breaks saving too.
pub fn request_fatal_shutdown(reason: &str) {
    steel_utils::fatal!("Unrecoverable storage failure, stopping the server: {reason}");

    if let Some(token) = FATAL_SHUTDOWN.get() {
        token.cancel();
    } else {
        steel_utils::fatal!("No shutdown token registered; stop the server manually");
    }
}

/// Whether a fatal shutdown is already in progress. Call sites use this to stay
/// quiet instead of repeating the same condition once per chunk.
pub fn fatal_shutdown_requested() -> bool {
    FATAL_SHUTDOWN
        .get()
        .is_some_and(CancellationToken::is_cancelled)
}
