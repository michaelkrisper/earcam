//! Turns "terminate" requests from outside the window into a clean quit, so the WiFi is
//! switched back even when EarCam is stopped from a terminal.
//!
//! - Unix: SIGINT, SIGTERM and SIGHUP are blocked in every thread and collected by one
//!   thread with `sigwait`. [`QuitSignals::block`] must run before any other thread is
//!   spawned, or that thread would still receive them and die with the default action.
//! - Windows: a console control handler. For Ctrl+C the process simply carries on; for
//!   closing the console window, logoff and shutdown Windows kills the process as soon as
//!   the handler returns, so it waits (up to 5 s, Windows' own limit) for [`finished`].

/// Signals blocked but not yet handled; see [`QuitSignals::spawn`].
pub struct QuitSignals {
    #[cfg(unix)]
    set: libc::sigset_t,
}

impl QuitSignals {
    /// Blocks the quit signals in this thread and every thread it spawns later.
    pub fn block() -> QuitSignals {
        #[cfg(unix)]
        unsafe {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
                libc::sigaddset(&mut set, sig);
            }
            libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
            QuitSignals { set }
        }
        #[cfg(windows)]
        QuitSignals {}
    }

    /// Calls `on_quit` whenever a quit is requested.
    pub fn spawn(self, on_quit: impl Fn() + Send + 'static) {
        #[cfg(unix)]
        std::thread::spawn(move || {
            loop {
                let mut sig = 0;
                if unsafe { libc::sigwait(&self.set, &mut sig) } == 0 {
                    on_quit();
                }
            }
        });
        #[cfg(windows)]
        windows::install(Box::new(on_quit));
    }
}

/// Reports that cleanup is done, releasing a Windows console handler waiting on it.
pub fn finished() {
    #[cfg(windows)]
    windows::FINISHED.store(true, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(windows)]
mod windows {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::TRUE;
    use windows_sys::Win32::System::Console::{
        CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler,
    };
    use windows_sys::core::BOOL;

    pub static FINISHED: AtomicBool = AtomicBool::new(false);
    static ON_QUIT: OnceLock<Mutex<Box<dyn Fn() + Send>>> = OnceLock::new();

    pub fn install(on_quit: Box<dyn Fn() + Send>) {
        if ON_QUIT.set(Mutex::new(on_quit)).is_ok() {
            unsafe { SetConsoleCtrlHandler(Some(handler), TRUE) };
        }
    }

    unsafe extern "system" fn handler(event: u32) -> BOOL {
        if let Some(on_quit) = ON_QUIT.get() {
            (on_quit.lock().unwrap())();
        }
        if event != CTRL_C_EVENT && event != CTRL_BREAK_EVENT {
            let deadline = Instant::now() + Duration::from_millis(4900);
            while !FINISHED.load(Ordering::SeqCst) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        TRUE
    }
}
