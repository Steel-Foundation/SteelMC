//! Process shutdown signal handling.
//!
//! Every handled signal cancels the server's [`CancellationToken`], the single
//! entry point into the shutdown that persists world and player data.

use std::sync::OnceLock;

use tokio_util::sync::CancellationToken;

static CANCEL_TOKEN: OnceLock<CancellationToken> = OnceLock::new();

/// Installs the shutdown signal handlers for the running platform.
///
/// Unix covers `SIGINT`, `SIGTERM` and `SIGHUP`; Windows covers `Ctrl-C`,
/// `Ctrl-Break`, console close, logoff and system shutdown.
pub fn install(cancel_token: CancellationToken) {
    if CANCEL_TOKEN.set(cancel_token).is_err() {
        log::error!("Shutdown signal handler is already installed");
        return;
    }

    if let Err(error) = platform::install() {
        log::error!("Failed to install shutdown signal handler: {error}");
    }
}

/// Reports that world and player data have been persisted. Windows blocks the
/// console control handler until this is called, so it must run on every exit path.
pub fn cleanup_finished() {
    platform::cleanup_finished();
}

/// Cancels the server, ignoring repeat signals so a second one cannot disturb a
/// shutdown that is already saving.
fn request_shutdown(source: &str) {
    let Some(cancel_token) = CANCEL_TOKEN.get() else {
        return;
    };
    if cancel_token.is_cancelled() {
        return;
    }

    log::info!("Received {source}; shutting down gracefully");
    cancel_token.cancel();
}

#[cfg(unix)]
mod platform {
    use super::request_shutdown;

    /// The `termination` feature extends `ctrlc`'s default `SIGINT` handling to
    /// `SIGTERM` and `SIGHUP`.
    pub fn install() -> Result<(), ctrlc::Error> {
        ctrlc::set_handler(|| request_shutdown("shutdown signal"))
    }

    /// Unix handlers never hold the process open, so there is nothing to release.
    pub const fn cleanup_finished() {}
}

#[cfg(windows)]
mod platform {
    use std::io;
    use std::time::Duration;

    use steel_utils::locks::{SyncCondvar, SyncMutex};
    use windows_sys::Win32::Foundation::{FALSE, TRUE};
    use windows_sys::Win32::System::Console::{
        CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT,
        SetConsoleCtrlHandler,
    };
    use windows_sys::core::BOOL;

    use super::request_shutdown;

    /// How long the console control handler waits for the shutdown to finish before
    /// letting Windows kill the process. Windows enforces its own ceiling on top of
    /// this (about five seconds, `HKCU\Control Panel\Desktop\WaitToKillAppTimeout`).
    const CLEANUP_GRACE: Duration = Duration::from_secs(10);

    static CLEANUP_DONE: SyncMutex<bool> = SyncMutex::new(false);
    static CLEANUP_SIGNAL: SyncCondvar = SyncCondvar::new();

    pub fn install() -> Result<(), io::Error> {
        // SAFETY: `console_ctrl_handler` has the `PHANDLER_ROUTINE` signature and
        // `'static` lifetime, so it stays valid for every call Windows can make.
        let installed = unsafe { SetConsoleCtrlHandler(Some(console_ctrl_handler), TRUE) };
        if installed == FALSE {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn cleanup_finished() {
        *CLEANUP_DONE.lock() = true;
        CLEANUP_SIGNAL.notify_all();
    }

    /// Windows runs this on its own thread and, for the close, logoff and shutdown
    /// events, kills the process once it returns. Blocking here is what keeps the
    /// world save from being cut short.
    unsafe extern "system" fn console_ctrl_handler(ctrl_type: u32) -> BOOL {
        let source = match ctrl_type {
            CTRL_C_EVENT => "Ctrl-C",
            CTRL_BREAK_EVENT => "Ctrl-Break",
            CTRL_CLOSE_EVENT => "console close",
            CTRL_LOGOFF_EVENT => "user logoff",
            CTRL_SHUTDOWN_EVENT => "system shutdown",
            _ => return FALSE,
        };

        request_shutdown(source);
        wait_for_cleanup(source);
        TRUE
    }

    fn wait_for_cleanup(source: &str) {
        let mut cleanup_done = CLEANUP_DONE.lock();
        let result = CLEANUP_SIGNAL.wait_while_for(&mut cleanup_done, |done| !*done, CLEANUP_GRACE);
        if result.timed_out() {
            log::error!(
                "Server data was still being saved {CLEANUP_GRACE:?} after {source}; the process may be killed before it finishes"
            );
        }
    }
}
