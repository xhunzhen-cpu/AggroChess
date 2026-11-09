//! Small OS helpers implemented without third-party crates:
//! stop flag + SIGINT/SIGTERM/SIGHUP handler, child-process hardening,
//! process-group kill, EPIPE-safe stdout/stderr output, civil date, hostname.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static STOP: AtomicBool = AtomicBool::new(false);
static STDOUT_BROKEN: AtomicBool = AtomicBool::new(false);

/// Write one chunk to stdout, ignoring errors. Rust ignores SIGPIPE, so `println!`
/// would panic when the reader of our stdout is gone (e.g. `| tee log` killed by the
/// same Ctrl-C); after the first failure stdout is no longer written and
/// `stdout_broken()` reports it.
pub fn out_str(s: &str) {
    if STDOUT_BROKEN.load(Ordering::Relaxed) {
        return;
    }
    let mut o = std::io::stdout().lock();
    if o.write_all(s.as_bytes()).and_then(|_| o.flush()).is_err() {
        STDOUT_BROKEN.store(true, Ordering::Relaxed);
    }
}

/// True once a write to stdout failed (closed pipe).
pub fn stdout_broken() -> bool {
    STDOUT_BROKEN.load(Ordering::Relaxed)
}

/// Write one chunk to stderr, ignoring errors.
pub fn err_str(s: &str) {
    let mut e = std::io::stderr().lock();
    let _ = e.write_all(s.as_bytes()).and_then(|_| e.flush());
}

/// True once SIGINT/SIGTERM/SIGHUP was received (or `request_stop` was called).
pub fn stop_requested() -> bool {
    STOP.load(Ordering::Relaxed)
}

pub fn request_stop() {
    STOP.store(true, Ordering::Relaxed);
}

#[cfg(target_os = "linux")]
mod ffi {
    extern "C" {
        pub fn signal(signum: i32, handler: usize) -> usize;
        pub fn prctl(option: i32, ...) -> i32;
        pub fn _exit(code: i32) -> !;
        pub fn getppid() -> i32;
        pub fn kill(pid: i32, sig: i32) -> i32;
    }
}

/// SIGKILL every process in the process group `pgid` (= the pid of a child started
/// with `harden_child`, which makes it a group leader). Errors (e.g. ESRCH when the
/// group is already empty) are ignored.
pub fn kill_process_group(pgid: u32) {
    #[cfg(target_os = "linux")]
    {
        let p = pgid as i32;
        if p > 1 {
            unsafe {
                ffi::kill(-p, 9);
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = pgid;
}

#[cfg(target_os = "linux")]
extern "C" fn on_signal(_sig: i32) {
    // async-signal-safe: atomics + _exit only
    if STOP.swap(true, Ordering::SeqCst) {
        // second signal: give up immediately; children die via PDEATHSIG
        unsafe { ffi::_exit(130) }
    }
}

/// Install handlers so the first Ctrl-C / SIGTERM requests a graceful stop
/// (running games are abandoned, engines are killed, summary is written) and
/// a second one exits immediately.
pub fn install_signal_handlers() {
    #[cfg(target_os = "linux")]
    unsafe {
        let h = on_signal as extern "C" fn(i32) as usize;
        ffi::signal(2, h); // SIGINT
        ffi::signal(15, h); // SIGTERM
        ffi::signal(1, h); // SIGHUP
    }
}

/// Configure a `Command` so the engine child (a) lives in its own process group
/// (terminal Ctrl-C reaches only the arena, which then shuts engines down in an
/// orderly way) and (b) receives SIGKILL if the spawning thread/process dies.
pub fn harden_child(cmd: &mut std::process::Command) {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
        let parent = std::process::id() as i32;
        unsafe {
            cmd.pre_exec(move || {
                // PR_SET_PDEATHSIG = 1, SIGKILL = 9
                ffi::prctl(1, 9 as std::os::raw::c_ulong);
                // parent already gone before prctl took effect?
                if ffi::getppid() != parent {
                    ffi::_exit(1);
                }
                Ok(())
            });
        }
    }
}

/// ISO-like UTC timestamp `YYYY-MM-DDTHH:MM:SSZ`.
pub fn now_iso_utc() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (y, m, d) = civil_from_days((secs / 86400) as i64);
    let rem = secs % 86400;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, rem / 3600, (rem / 60) % 60, rem % 60)
}

/// Howard Hinnant's days-from-civil inverse.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "?".to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn civil() {
        assert_eq!(super::civil_from_days(0), (1970, 1, 1));
        assert_eq!(super::civil_from_days(20764), (2026, 11, 7));
        assert_eq!(super::civil_from_days(11016), (2000, 2, 29));
    }
}
