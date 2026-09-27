//! Minimal `log` backend: stderr on the desktop, logcat on Android under the tag `EarCam`
//! (`adb logcat -s EarCam`).

use log::{Level, LevelFilter, Log, Metadata, Record};

struct Logger;

static LOGGER: Logger = Logger;

/// Installs the logger; later calls are ignored.
pub fn init() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(LevelFilter::Info);
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        write(record.level(), &record.args().to_string());
    }

    fn flush(&self) {}
}

#[cfg(not(target_os = "android"))]
fn write(level: Level, message: &str) {
    match level {
        Level::Error | Level::Warn => eprintln!("{level}: {message}"),
        _ => eprintln!("{message}"),
    }
}

#[cfg(target_os = "android")]
fn write(level: Level, message: &str) {
    use std::ffi::CString;
    let priority = match level {
        Level::Error => ndk_sys::android_LogPriority::ANDROID_LOG_ERROR,
        Level::Warn => ndk_sys::android_LogPriority::ANDROID_LOG_WARN,
        Level::Info => ndk_sys::android_LogPriority::ANDROID_LOG_INFO,
        Level::Debug => ndk_sys::android_LogPriority::ANDROID_LOG_DEBUG,
        Level::Trace => ndk_sys::android_LogPriority::ANDROID_LOG_VERBOSE,
    };
    let text = CString::new(message.replace('\0', " ")).unwrap_or_default();
    unsafe { ndk_sys::__android_log_write(priority.0 as i32, c"EarCam".as_ptr(), text.as_ptr()) };
}
