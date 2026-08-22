//! Process shutdown signal handling.
//!
//! Every handled signal cancels the server's [`CancellationToken`], the single
//! entry point into the shutdown that persists world and player data.

use tokio_util::sync::CancellationToken;

use platform::ShutdownSignals;

/// Spawns the listener that cancels `cancel_token` on the first shutdown signal.
///
/// Unix covers `SIGINT`, `SIGTERM` and `SIGHUP`; Windows covers `Ctrl-C`,
/// `Ctrl-Break`, console close, logoff and system shutdown.
pub fn install(cancel_token: CancellationToken) {
    tokio::spawn(async move {
        let mut signals = match ShutdownSignals::new() {
            Ok(signals) => signals,
            Err(error) => {
                log::error!("Failed to listen for shutdown signals: {error}");
                return;
            }
        };

        let source = signals.recv().await;
        log::info!("Received {source}; shutting down gracefully");
        cancel_token.cancel();

        // Windows kills the process outright when a console event finds no
        // listener, so keep the streams alive for the rest of the shutdown
        // instead of letting a second Ctrl-C interrupt the save.
        loop {
            signals.recv().await;
        }
    });
}

#[cfg(unix)]
mod platform {
    use std::io;

    use tokio::signal::unix::{Signal, SignalKind, signal};

    /// The termination signals a Unix service is expected to shut down on.
    pub struct ShutdownSignals {
        interrupt: Signal,
        terminate: Signal,
        hangup: Signal,
    }

    impl ShutdownSignals {
        pub fn new() -> io::Result<Self> {
            Ok(Self {
                interrupt: signal(SignalKind::interrupt())?,
                terminate: signal(SignalKind::terminate())?,
                hangup: signal(SignalKind::hangup())?,
            })
        }

        pub async fn recv(&mut self) -> &'static str {
            tokio::select! {
                _ = self.interrupt.recv() => "SIGINT",
                _ = self.terminate.recv() => "SIGTERM",
                _ = self.hangup.recv() => "SIGHUP",
            }
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::io;

    use tokio::signal::windows::{
        CtrlBreak, CtrlC, CtrlClose, CtrlLogoff, CtrlShutdown, ctrl_break, ctrl_c, ctrl_close,
        ctrl_logoff, ctrl_shutdown,
    };

    /// Every console control event, including the window's close button.
    ///
    /// Returning from a console control handler for the close, logoff and shutdown
    /// events terminates the process, so tokio parks that handler thread instead of
    /// returning. That is what keeps the world save from being cut short, and it
    /// only holds while these streams are alive.
    pub struct ShutdownSignals {
        ctrl_c: CtrlC,
        ctrl_break: CtrlBreak,
        ctrl_close: CtrlClose,
        ctrl_logoff: CtrlLogoff,
        ctrl_shutdown: CtrlShutdown,
    }

    impl ShutdownSignals {
        pub fn new() -> io::Result<Self> {
            Ok(Self {
                ctrl_c: ctrl_c()?,
                ctrl_break: ctrl_break()?,
                ctrl_close: ctrl_close()?,
                ctrl_logoff: ctrl_logoff()?,
                ctrl_shutdown: ctrl_shutdown()?,
            })
        }

        pub async fn recv(&mut self) -> &'static str {
            tokio::select! {
                _ = self.ctrl_c.recv() => "Ctrl-C",
                _ = self.ctrl_break.recv() => "Ctrl-Break",
                _ = self.ctrl_close.recv() => "console close",
                _ = self.ctrl_logoff.recv() => "user logoff",
                _ = self.ctrl_shutdown.recv() => "system shutdown",
            }
        }
    }
}
