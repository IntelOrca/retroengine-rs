//! Ctrl-C/SIGTERM handling for backends without an event pump.
//!
//! The SDL3 backend turns SIGINT/SIGTERM into quit events through SDL's own signal handling.
//! Headless runs have no event pump, so [`install`] registers process-wide handlers that set
//! the flag [`requested`] reports. The engine CLI polls that flag once per frame and stops
//! cleanly (flush saves and print the run summary) instead of dying mid-frame.
//!
//! Unix uses SIGINT/SIGTERM handlers; Windows uses a console control handler for Ctrl-C and
//! Ctrl-Break. On other platforms [`install`] returns `false` and [`requested`] always reports
//! `false`, so the process keeps the OS default (terminate) behaviour.

use std::sync::atomic::{AtomicBool, Ordering};

static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Whether a quit signal (Ctrl-C/SIGTERM) has arrived since process start.
#[must_use]
pub fn requested() -> bool {
    QUIT_REQUESTED.load(Ordering::SeqCst)
}

/// Registers the quit-signal handlers when the platform supports them.
///
/// Returns `true` when [`requested`] observes SIGINT/SIGTERM. Repeated calls replace the
/// handlers with the same ones and are harmless.
#[must_use]
pub fn install() -> bool {
    imp::install()
}

#[cfg(unix)]
mod imp {
    use std::sync::atomic::Ordering;

    extern "C" fn handle(_signal: libc::c_int) {
        super::QUIT_REQUESTED.store(true, Ordering::SeqCst);
    }

    pub(super) fn install() -> bool {
        // SAFETY: `handle` only stores to an atomic, which is async-signal-safe, and `signal`
        // is called with valid signal numbers and a valid handler pointer.
        unsafe {
            libc::signal(libc::SIGINT, handle as *const () as libc::sighandler_t);
            libc::signal(libc::SIGTERM, handle as *const () as libc::sighandler_t);
        }
        true
    }
}

#[cfg(windows)]
mod imp {
    use std::sync::atomic::Ordering;

    const CTRL_C_EVENT: u32 = 0;
    const CTRL_BREAK_EVENT: u32 = 1;

    type HandlerRoutine = unsafe extern "system" fn(ctrl_type: u32) -> i32;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetConsoleCtrlHandler(handler_routine: Option<HandlerRoutine>, add: i32) -> i32;
    }

    unsafe extern "system" fn handle(ctrl_type: u32) -> i32 {
        if ctrl_type == CTRL_C_EVENT || ctrl_type == CTRL_BREAK_EVENT {
            super::QUIT_REQUESTED.store(true, Ordering::SeqCst);
            1
        } else {
            // Let Windows' default handler terminate on close/logoff/shutdown events.
            0
        }
    }

    pub(super) fn install() -> bool {
        // SAFETY: `handle` only stores to an atomic and reports whether it handled the event.
        unsafe { SetConsoleCtrlHandler(Some(handle), 1) != 0 }
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    pub(super) fn install() -> bool {
        false
    }
}
