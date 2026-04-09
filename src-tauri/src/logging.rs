use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::platform;

static LOG_FILE: Mutex<Option<PathBuf>> = Mutex::new(None);
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Initialize logging. Call once at startup.
pub fn init(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);

    if !enabled {
        return;
    }

    let dir = log_dir();
    let _ = fs::create_dir_all(&dir);

    // Keep last 5 log files, rotate on startup
    let path = dir.join("roux.log");
    for i in (1..5).rev() {
        let old = dir.join(format!("roux.{}.log", i));
        let new = dir.join(format!("roux.{}.log", i + 1));
        let _ = fs::rename(&old, &new);
    }
    if path.exists() {
        let _ = fs::rename(&path, dir.join("roux.1.log"));
    }

    *LOG_FILE.lock().unwrap() = Some(path.clone());
    log(&format!("=== Roux started at {} ===", chrono_now()));
    log(&format!("Log file: {}", path.display()));
    log(&format!("OS: {} {}", std::env::consts::OS, std::env::consts::ARCH));
    log(&format!("SHELL: {}", std::env::var("SHELL").unwrap_or_else(|_| "(unset)".into())));
}

/// Enable or disable logging at runtime (e.g. when settings change).
pub fn set_enabled(enabled: bool) {
    let was_enabled = ENABLED.swap(enabled, Ordering::Relaxed);
    if enabled && !was_enabled {
        // Turning on — initialize the log file if not already done
        let guard = LOG_FILE.lock().unwrap();
        if guard.is_none() {
            drop(guard);
            init(true);
        }
    }
}

fn chrono_now() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", hours, mins, s)
}

/// Write a line to the log file (no-op if logging is disabled).
pub fn log(msg: &str) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let guard = LOG_FILE.lock().unwrap();
    if let Some(path) = guard.as_ref() {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "[{}] {}", chrono_now(), msg);
        }
    }
    // Also write to stderr for dev console
    eprintln!("[roux] {}", msg);
}

/// Return the path to the current log file.
pub fn log_path() -> Option<PathBuf> {
    let path = log_dir().join("roux.log");
    Some(path)
}

/// Return whether logging is currently enabled.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Convenience macro for formatted logging.
#[macro_export]
macro_rules! rlog {
    ($($arg:tt)*) => {
        $crate::logging::log(&format!($($arg)*))
    };
}

fn log_dir() -> PathBuf {
    platform::log_dir()
}
