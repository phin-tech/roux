use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

static LOG_FILE: Mutex<Option<PathBuf>> = Mutex::new(None);
static ENABLED: AtomicBool = AtomicBool::new(false);
/// Initialize logging. Call once at startup.
pub fn init(enabled: bool) {
    // Install the panic hook unconditionally so panic messages always land
    // in roux.log, even when settings have logging disabled. Stderr from a
    // launchd-spawned `.app` bundle goes to /dev/null, so without this hook
    // a panic on the FFI boundary aborts the process with no recoverable
    // diagnostic.
    install_panic_hook();

    ENABLED.store(enabled, Ordering::Relaxed);

    if !enabled {
        return;
    }

    rotate_existing_logs();
    initialize_log_file();
}

/// Enable or disable logging at runtime (e.g. when settings change).
pub fn set_enabled(enabled: bool) {
    let was_enabled = ENABLED.swap(enabled, Ordering::Relaxed);
    if enabled && !was_enabled {
        // Turning on — initialize the log file if not already done.
        // Don't go through `init(true)` here because that would rotate
        // any existing `roux.log` to `roux.1.log`, hiding panic data
        // captured by the panic hook while logging was disabled. Rotation
        // is reserved for actual process startup.
        let guard = LOG_FILE.lock().unwrap();
        if guard.is_none() {
            drop(guard);
            initialize_log_file();
        }
    }
}

/// Rotate existing log files. Called once at process startup.
fn rotate_existing_logs() {
    let dir = log_dir();
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("roux.log");
    // Keep last 5 log files
    for i in (1..5).rev() {
        let old = dir.join(format!("roux.{}.log", i));
        let new = dir.join(format!("roux.{}.log", i + 1));
        let _ = fs::rename(&old, &new);
    }
    if path.exists() {
        let _ = fs::rename(&path, dir.join("roux.1.log"));
    }
}

/// Set up the in-memory `LOG_FILE` pointer and write the session-start
/// header lines. Does not rotate — safe to call from `set_enabled` when
/// logging is toggled on at runtime.
fn initialize_log_file() {
    let dir = log_dir();
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("roux.log");
    *LOG_FILE.lock().unwrap() = Some(path.clone());
    log(&format!("=== Roux started at {} ===", chrono_now()));
    log(&format!("Log file: {}", path.display()));
    log(&format!("OS: {} {}", std::env::consts::OS, std::env::consts::ARCH));
    log(&format!("SHELL: {}", std::env::var("SHELL").unwrap_or_else(|_| "(unset)".into())));
}

fn chrono_now() -> String {
    let dur =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
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
#[allow(dead_code)]
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
    crate::paths::roux_config_dir().join("logs")
}

/// Install a process-wide panic hook that writes panic info to roux.log
/// regardless of the `ENABLED` flag. Idempotent.
fn install_panic_hook() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    if INSTALLED.set(()).is_err() {
        return;
    }
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".into());
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string panic payload>".into());
        let thread = std::thread::current()
            .name()
            .map(String::from)
            .unwrap_or_else(|| format!("{:?}", std::thread::current().id()));
        log_unconditional(&format!("PANIC thread={thread} at {location}: {payload}"));
        // The previous (default) hook handles stderr output for dev/CLI runs.
        prev(info);
    }));
}

/// Append `msg` to roux.log regardless of the `ENABLED` flag.
///
/// Used by the panic hook so panics always land in the log even when the
/// user has logging turned off, and exposed for callers like
/// `tray::handle_menu_event`'s `catch_unwind` recovery path that need to
/// record diagnostics whose loss would defeat the purpose of catching.
///
/// Does not write to stderr — the default panic hook (chained from
/// `install_panic_hook`) already prints to stderr in dev/CLI runs, and
/// production `.app` bundles route stderr to /dev/null.
pub fn log_unconditional(msg: &str) {
    let dir = log_dir();
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("roux.log");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{}] {}", chrono_now(), msg);
    }
}
