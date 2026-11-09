//! UCI engine process wrapper.
//!
//! Each engine runs as a child process with piped stdin/stdout (stderr is
//! discarded). A dedicated reader thread pushes every stdout line, time-stamped
//! at the moment it was read, into a *bounded* channel so that all reads can use
//! `recv_timeout`, elapsed times are measured independently of how quickly the
//! worker thread gets scheduled, and an engine that floods its output cannot
//! make the arena buffer without limit (the reader blocks, then the engine blocks
//! on its pipe). Every wait checks its deadline and the stop flag on each line.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};

use std::thread;
use std::time::{Duration, Instant};

use crate::sys;

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub name: String,
    pub path: String,
    pub args: Vec<String>,
    /// (UCI option name, value) sent in this order after `uciok`.
    pub options: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
}

impl EngineConfig {
    pub fn new(name: &str, path: &str) -> EngineConfig {
        EngineConfig { name: name.to_string(), path: path.to_string(), args: Vec::new(), options: Vec::new(), cwd: None }
    }
}

#[derive(Debug, Clone)]
pub struct Line {
    pub at: Instant,
    pub text: String,
}

#[derive(Debug)]
pub enum Recv {
    Line(Line),
    Timeout,
    Closed,
}

#[derive(Debug, Clone)]
pub struct UciOption {
    pub name: String,
    pub kind: String,
    pub default: Option<String>,
    pub min: Option<i64>,
    pub max: Option<i64>,
    pub vars: Vec<String>,
}

impl UciOption {
    /// Parse an `option name ... type ...` line.
    pub fn parse(line: &str) -> Option<UciOption> {
        let toks: Vec<&str> = line.split_whitespace().collect();
        if toks.first() != Some(&"option") {
            return None;
        }
        let mut name: Vec<&str> = Vec::new();
        let mut kind = String::new();
        let mut default: Option<Vec<&str>> = None;
        let mut min = None;
        let mut max = None;
        let mut vars: Vec<Vec<&str>> = Vec::new();
        let mut key = "";
        for &t in &toks[1..] {
            // option names may contain spaces: everything up to `type` is the name
            match t {
                "name" if key.is_empty() => key = "name",
                "type" if key == "name" || kind.is_empty() => key = "type",
                "default" if key != "name" => {
                    key = "default";
                    default = Some(Vec::new());
                }
                "min" if key != "name" => key = "min",
                "max" if key != "name" => key = "max",
                "var" if key != "name" => {
                    key = "var";
                    vars.push(Vec::new());
                }
                _ => match key {
                    "name" => name.push(t),
                    "type" => kind = t.to_string(),
                    "default" => default.get_or_insert_with(Vec::new).push(t),
                    "min" => min = t.parse::<i64>().ok(),
                    "max" => max = t.parse::<i64>().ok(),
                    "var" => {
                        if let Some(v) = vars.last_mut() {
                            v.push(t)
                        }
                    }
                    _ => {}
                },
            }
        }
        if name.is_empty() {
            return None;
        }
        let default = default.map(|d| {
            let s = d.join(" ");
            if s == "<empty>" {
                String::new()
            } else {
                s
            }
        });
        Some(UciOption {
            name: name.join(" "),
            kind,
            default,
            min,
            max,
            vars: vars.into_iter().map(|v| v.join(" ")).collect(),
        })
    }

    pub fn describe(&self) -> String {
        let mut s = format!("{} ({}", self.name, self.kind);
        if let Some(d) = &self.default {
            s.push_str(&format!(", default '{}'", d));
        }
        if let (Some(a), Some(b)) = (self.min, self.max) {
            s.push_str(&format!(", {}..{}", a, b));
        }
        if !self.vars.is_empty() {
            s.push_str(&format!(", vars [{}]", self.vars.join(", ")));
        }
        s.push(')');
        s
    }
}

/// Capacity (lines) of the reader thread -> worker channel.
pub const LINE_CHANNEL_CAP: usize = 4096;
/// `wait_for` gives up after this many lines without the awaited reply (a flooding engine).
pub const MAX_WAIT_LINES: usize = 100_000;

pub struct Engine {
    pub name: String,
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<Line>,
    reaped: bool,
    /// Set when the process died, was killed, or is in an unknown state; a dead
    /// engine must not be reused.
    pub dead: bool,
    pub id_name: Option<String>,
    pub id_author: Option<String>,
    pub options: Vec<UciOption>,
}

pub enum WaitError {
    Timeout(Vec<Line>),
    Closed(Vec<Line>),
    /// More than MAX_WAIT_LINES lines without the awaited reply.
    Flood(usize),
}

impl WaitError {
    pub fn describe(&self, what: &str) -> String {
        match self {
            WaitError::Timeout(l) => format!("timeout waiting for {} ({} other lines received)", what, l.len()),
            WaitError::Flood(n) => format!("no {} after {} output lines (engine floods its output)", what, n),
            WaitError::Closed(l) => match l.last() {
                Some(last) => format!("engine exited while waiting for {} (last output: '{}')", what, last.text),
                None => format!("engine exited while waiting for {}", what),
            },
        }
    }
}

impl Engine {
    /// Spawn the process (no UCI handshake yet).
    pub fn spawn(cfg: &EngineConfig) -> Result<Engine, String> {
        let mut cmd = Command::new(&cfg.path);
        cmd.args(&cfg.args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        if let Some(dir) = &cfg.cwd {
            cmd.current_dir(dir);
        }
        sys::harden_child(&mut cmd);
        let mut child = cmd.spawn().map_err(|e| format!("cannot start '{}': {}", cfg.path, e))?;
        let stdout = child.stdout.take().ok_or("no stdout pipe")?;
        let stdin = child.stdin.take();
        let (tx, rx) = mpsc::sync_channel::<Line>(LINE_CHANNEL_CAP);
        let tname = format!("rd-{}", cfg.name);
        let spawn_res = thread::Builder::new().name(tname).stack_size(64 * 1024).spawn(move || {
            let mut r = BufReader::with_capacity(64 * 1024, stdout);
            let mut buf: Vec<u8> = Vec::with_capacity(512);
            loop {
                buf.clear();
                match r.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let at = Instant::now();
                        while matches!(buf.last(), Some(b'\n') | Some(b'\r')) {
                            buf.pop();
                        }
                        let text = String::from_utf8_lossy(&buf).into_owned();
                        if tx.send(Line { at, text }).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        if let Err(e) = spawn_res {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("cannot spawn reader thread: {}", e));
        }
        Ok(Engine {
            name: cfg.name.clone(),
            child,
            stdin,
            rx,
            reaped: false,
            dead: false,
            id_name: None,
            id_author: None,
            options: Vec::new(),
        })
    }

    /// Spawn + `uci`/`uciok` + options + `isready`/`readyok`.
    pub fn start(cfg: &EngineConfig, uciok_timeout: Duration, ready_timeout: Duration) -> Result<Engine, String> {
        let mut e = Engine::spawn(cfg)?;
        e.handshake(uciok_timeout)?;
        for (k, v) in &cfg.options {
            e.set_option(k, v)?;
        }
        e.isready(ready_timeout)?;
        Ok(e)
    }

    /// Write one or more lines (a trailing newline is added).
    pub fn send(&mut self, text: &str) -> Result<(), String> {
        let stdin = match self.stdin.as_mut() {
            Some(s) => s,
            None => return Err("stdin closed".into()),
        };
        let mut s = String::with_capacity(text.len() + 1);
        s.push_str(text);
        s.push('\n');
        let r = stdin.write_all(s.as_bytes()).and_then(|_| stdin.flush());
        r.map_err(|e| {
            self.dead = true;
            format!("write to engine failed: {}", e)
        })
    }

    pub fn set_option(&mut self, name: &str, value: &str) -> Result<(), String> {
        self.send(&format!("setoption name {} value {}", name, value))
    }

    pub fn recv(&self, timeout: Duration) -> Recv {
        match self.rx.recv_timeout(timeout) {
            Ok(l) => Recv::Line(l),
            Err(RecvTimeoutError::Timeout) => Recv::Timeout,
            Err(RecvTimeoutError::Disconnected) => Recv::Closed,
        }
    }

    pub fn try_recv(&self) -> Recv {
        match self.rx.try_recv() {
            Ok(l) => Recv::Line(l),
            Err(TryRecvError::Empty) => Recv::Timeout,
            Err(TryRecvError::Disconnected) => Recv::Closed,
        }
    }

    /// Discard everything already received (bounded: lines read after this call
    /// started end the drain, so a flooding engine cannot keep it busy forever).
    pub fn drain(&self) -> Vec<Line> {
        let start = Instant::now();
        let mut v = Vec::new();
        while let Ok(l) = self.rx.try_recv() {
            if l.at > start || v.len() > LINE_CHANNEL_CAP {
                break;
            }
            v.push(l);
        }
        v
    }

    /// Read lines until `pred` matches (the matching line is the last element).
    /// Lines read from the pipe before the deadline are still examined after it; the
    /// first line read after the deadline (or an empty queue) ends the wait.
    pub fn wait_for<F: Fn(&str) -> bool>(&mut self, pred: F, timeout: Duration) -> Result<Vec<Line>, WaitError> {
        let deadline = Instant::now() + timeout;
        let mut seen = Vec::new();
        loop {
            if seen.len() >= MAX_WAIT_LINES {
                return Err(WaitError::Flood(seen.len()));
            }
            let now = Instant::now();
            let item = if now >= deadline {
                match self.try_recv() {
                    Recv::Line(l) if l.at <= deadline => Recv::Line(l),
                    Recv::Line(_) | Recv::Timeout => return Err(WaitError::Timeout(seen)),
                    Recv::Closed => Recv::Closed,
                }
            } else {
                self.recv(deadline - now)
            };
            match item {
                Recv::Line(l) => {
                    let hit = pred(l.text.trim());
                    seen.push(l);
                    if hit {
                        return Ok(seen);
                    }
                }
                Recv::Timeout => {}
                Recv::Closed => {
                    self.dead = true;
                    return Err(WaitError::Closed(seen));
                }
            }
        }
    }

    /// `uci` → `uciok`, recording id and options.
    pub fn handshake(&mut self, timeout: Duration) -> Result<(), String> {
        self.send("uci")?;
        let lines = self.wait_for(|l| l == "uciok", timeout).map_err(|e| e.describe("uciok"))?;
        for l in &lines {
            let t = l.text.trim();
            if let Some(n) = t.strip_prefix("id name ") {
                self.id_name = Some(n.trim().to_string());
            } else if let Some(a) = t.strip_prefix("id author ") {
                self.id_author = Some(a.trim().to_string());
            } else if t.starts_with("option ") {
                if let Some(o) = UciOption::parse(t) {
                    self.options.push(o);
                }
            }
        }
        Ok(())
    }

    pub fn isready(&mut self, timeout: Duration) -> Result<(), String> {
        self.send("isready")?;
        self.wait_for(|l| l == "readyok", timeout).map(|_| ()).map_err(|e| e.describe("readyok"))
    }

    pub fn has_option(&self, name: &str) -> bool {
        self.options.iter().any(|o| o.name.eq_ignore_ascii_case(name))
    }

    /// The process was just reaped: also SIGKILL whatever is left in its process
    /// group (helpers forked by a wrapper script, which PR_SET_PDEATHSIG does not
    /// cover). Done immediately after reaping, so the group id cannot have been reused.
    fn mark_reaped(&mut self) {
        if !self.reaped {
            self.reaped = true;
            sys::kill_process_group(self.child.id());
        }
    }

    pub fn is_alive(&mut self) -> bool {
        if self.reaped {
            return false;
        }
        match self.child.try_wait() {
            Ok(None) => true,
            Ok(Some(_)) => {
                self.mark_reaped();
                false
            }
            Err(_) => false,
        }
    }

    /// Exit status description if the process has exited.
    pub fn exit_status(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(Some(st)) => {
                self.mark_reaped();
                Some(st.to_string())
            }
            _ => None,
        }
    }

    /// Wait up to `timeout` for the process to exit on its own.
    pub fn wait_exit(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if !self.is_alive() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// SIGKILL the engine's whole process group (the engine is its own group
    /// leader, see `sys::harden_child`), so a hung engine started through a wrapper
    /// script does not survive as an orphan; then reap the direct child.
    pub fn kill(&mut self) {
        self.dead = true;
        self.stdin = None;
        if !self.reaped {
            sys::kill_process_group(self.child.id());
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.reaped = true;
        }
    }

    /// Polite shutdown: `quit`, wait up to `grace`, then kill.
    pub fn quit(&mut self, grace: Duration) {
        if !self.reaped {
            if !self.dead {
                let _ = self.send("quit");
            }
            self.stdin = None; // EOF also tells most engines to exit
            if !self.wait_exit(grace) {
                self.kill();
            }
        }
        self.dead = true;
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.quit(Duration::from_millis(300));
    }
}

// ---------------------------------------------------------------- search replies

#[derive(Debug, Clone)]
pub struct SearchReply {
    pub bestmove: String,
    pub ponder: Option<String>,
    /// Time the `bestmove` line was read from the pipe.
    pub at: Instant,
    /// Last reported score (mover's view, cp; mates encoded ±(100000-|n|)).
    pub score: Option<i32>,
    pub depth: Option<u32>,
    /// `info string book move` seen during this search.
    pub book: bool,
}

/// `Timeout` / `Crashed` carry the last few output lines (diagnostics).
#[derive(Debug)]
pub enum SearchOutcome {
    Reply(SearchReply),
    Timeout(Vec<String>),
    Crashed(Vec<String>),
    Aborted,
}

/// Last line of a diagnostics tail, for error messages.
pub fn last_line(lines: &[String]) -> String {
    match lines.last() {
        Some(l) => {
            let t: String = l.chars().take(160).collect();
            format!("last output: '{}'", t)
        }
        None => "no output".to_string(),
    }
}

pub const MATE_SCORE: i32 = 100_000;

pub fn encode_mate(n: i64) -> i32 {
    if n > 0 {
        MATE_SCORE - (n.min(MATE_SCORE as i64 - 1) as i32)
    } else if n < 0 {
        -(MATE_SCORE - ((-n).min(MATE_SCORE as i64 - 1) as i32))
    } else {
        -MATE_SCORE
    }
}

pub fn is_mate_score(s: i32) -> bool {
    s.abs() > MATE_SCORE - 1000
}

/// Update score/depth/book from one engine output line.
pub fn parse_info(line: &str, score: &mut Option<i32>, depth: &mut Option<u32>, book: &mut bool) {
    let t = line.trim();
    if !t.starts_with("info") {
        return;
    }
    let toks: Vec<&str> = t.split_whitespace().collect();
    if toks.first() != Some(&"info") {
        return;
    }
    if toks.get(1) == Some(&"string") {
        let rest = toks[2..].join(" ").to_ascii_lowercase();
        if rest.starts_with("book move") {
            *book = true;
        }
        return;
    }
    let mut multipv = 1i64;
    let mut sc: Option<i32> = None;
    let mut d: Option<u32> = None;
    let mut i = 1;
    while i < toks.len() {
        match toks[i] {
            "multipv" => {
                multipv = toks.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(1);
                i += 2;
            }
            "depth" => {
                d = toks.get(i + 1).and_then(|v| v.parse().ok());
                i += 2;
            }
            "score" => {
                match (toks.get(i + 1), toks.get(i + 2).and_then(|v| v.parse::<i64>().ok())) {
                    (Some(&"cp"), Some(v)) => sc = Some(v.clamp(-(MATE_SCORE as i64 - 1001), MATE_SCORE as i64 - 1001) as i32),
                    (Some(&"mate"), Some(v)) => sc = Some(encode_mate(v)),
                    _ => {}
                }
                i += 3;
            }
            "string" | "pv" => break,
            _ => i += 1,
        }
    }
    if multipv != 1 {
        return;
    }
    if let Some(s) = sc {
        *score = Some(s);
        if d.is_some() {
            *depth = d;
        }
    }
}

/// Read engine output until `bestmove` or until `deadline`.
/// The deadline and the stop flag are checked on every line (an engine that floods
/// `info` lines without ever sending `bestmove` still times out). Lines read from the
/// pipe before the deadline are processed even if the worker gets to them later.
pub fn wait_bestmove(eng: &mut Engine, deadline: Instant) -> SearchOutcome {
    let mut score = None;
    let mut depth = None;
    let mut book = false;
    let mut lines: Vec<String> = Vec::new();
    loop {
        if sys::stop_requested() {
            return SearchOutcome::Aborted;
        }
        let now = Instant::now();
        let item = if now >= deadline {
            match eng.try_recv() {
                Recv::Line(l) if l.at <= deadline => Recv::Line(l),
                Recv::Line(_) | Recv::Timeout => return SearchOutcome::Timeout(lines),
                Recv::Closed => Recv::Closed,
            }
        } else {
            match eng.try_recv() {
                Recv::Timeout => eng.recv((deadline - now).min(Duration::from_millis(100))),
                other => other,
            }
        };
        match item {
            Recv::Line(l) => {
                let t = l.text.trim();
                if t.starts_with("bestmove") {
                    let toks: Vec<&str> = t.split_whitespace().collect();
                    let bm = toks.get(1).copied().unwrap_or("").to_string();
                    let ponder = if toks.get(2) == Some(&"ponder") { toks.get(3).map(|s| s.to_string()) } else { None };
                    return SearchOutcome::Reply(SearchReply { bestmove: bm, ponder, at: l.at, score, depth, book });
                }
                parse_info(t, &mut score, &mut depth, &mut book);
                if lines.len() >= 8 {
                    lines.remove(0);
                }
                lines.push(l.text);
            }
            Recv::Timeout => {}
            Recv::Closed => {
                if sys::stop_requested() {
                    return SearchOutcome::Aborted;
                }
                eng.dead = true;
                return SearchOutcome::Crashed(lines);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_parse() {
        let o = UciOption::parse("option name Skill Level type spin default 20 min 0 max 20").unwrap();
        assert_eq!(o.name, "Skill Level");
        assert_eq!(o.kind, "spin");
        assert_eq!(o.default.as_deref(), Some("20"));
        assert_eq!((o.min, o.max), (Some(0), Some(20)));
        let o = UciOption::parse("option name Debug Log File type string default").unwrap();
        assert_eq!(o.name, "Debug Log File");
        assert_eq!(o.default.as_deref(), Some(""));
        let o = UciOption::parse("option name Style type combo default Normal var Solid var Normal var Risky").unwrap();
        assert_eq!(o.vars, vec!["Solid", "Normal", "Risky"]);
        let o = UciOption::parse("option name Clear Hash type button").unwrap();
        assert_eq!(o.kind, "button");
        assert!(o.default.is_none());
    }

    #[test]
    fn info_parse() {
        let (mut s, mut d, mut b) = (None, None, false);
        parse_info("info depth 12 seldepth 18 multipv 1 score cp 35 nodes 1000 pv e2e4 e7e5", &mut s, &mut d, &mut b);
        assert_eq!((s, d), (Some(35), Some(12)));
        parse_info("info depth 12 multipv 2 score cp -50 pv d2d4", &mut s, &mut d, &mut b);
        assert_eq!((s, d), (Some(35), Some(12)));
        parse_info("info depth 13 currmove e2e4 currmovenumber 1", &mut s, &mut d, &mut b);
        assert_eq!((s, d), (Some(35), Some(12)));
        parse_info("info depth 14 score mate 3 pv a1a8", &mut s, &mut d, &mut b);
        assert_eq!((s, d), (Some(99997), Some(14)));
        parse_info("info depth 15 score mate -2 pv a1a8", &mut s, &mut d, &mut b);
        assert_eq!(s, Some(-99998));
        parse_info("info score cp 10 lowerbound depth 9", &mut s, &mut d, &mut b);
        assert_eq!((s, d), (Some(10), Some(9)));
        assert!(!b);
        parse_info("info string book move", &mut s, &mut d, &mut b);
        assert!(b);
    }
}
