//! `ucitest` subcommand: scripted UCI conformance test of one engine.
//!
//! Prints PASS / FAIL / WARN per check; exits 1 if any check FAILed.
//! WARN marks behaviour that is questionable but not a protocol violation
//! (e.g. an engine exiting by design when a file option points nowhere).

use std::time::{Duration, Instant};

use cozy_chess::Board;

use crate::engine::{wait_bestmove, Engine, EngineConfig, SearchOutcome, SearchReply, UciOption};
use crate::san;

// balanced, quiet middlegame (no forced lines, unlikely to be in an opening book)
const MIDGAME_W: &str = "r2q1rk1/pb1nbppp/1pp1pn2/3p4/2PP4/1PN1PN2/P2BBPPP/R2Q1RK1 w - - 0 10";
const MIDGAME_B: &str = "r2q1rk1/pb1nbppp/1pp1pn2/3p4/2PP4/1PN1PN2/P2BBPPP/R2Q1RK1 b - - 0 10";
const COMPLEX: &str = "r2q1rk1/pb1nbppp/1pp1pn2/3p4/2PP4/1PN1PN2/P2BBPPP/R2Q1RK1 w - - 0 10";

struct Tester {
    cfg: EngineConfig,
    user_opts: Vec<(String, String)>,
    eng: Option<Engine>,
    pass: u32,
    fail: u32,
    warn: u32,
    failed_names: Vec<String>,
}

fn ms(d: Duration) -> u128 {
    d.as_millis()
}

/// The Human Think Time act can hold a reply back well past a fixed `go movetime`; when the
/// user turns it on, the fixed-time checks wait for the act (the match runner already
/// allows 60 s per fixed-time move), and overrun warnings are suppressed instead.
fn human_think_on(user_opts: &[(String, String)]) -> bool {
    user_opts
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("Human Think Time") && v != "0" && !v.eq_ignore_ascii_case("false") && !v.is_empty())
}

fn board_of(fen: Option<&str>, moves: &[&str]) -> Result<Board, String> {
    let mut b = match fen {
        Some(f) => Board::from_fen(f, false).map_err(|e| format!("bad test FEN '{}': {:?}", f, e))?,
        None => Board::default(),
    };
    for m in moves {
        let mv = san::parse_uci(&b, m).ok_or_else(|| format!("bad test move {}", m))?;
        b.play_unchecked(mv);
    }
    Ok(b)
}

fn position_cmd(fen: Option<&str>, moves: &[&str]) -> String {
    let mut s = match fen {
        Some(f) => format!("position fen {}", f),
        None => "position startpos".to_string(),
    };
    if !moves.is_empty() {
        s.push_str(" moves ");
        s.push_str(&moves.join(" "));
    }
    s
}

/// Values used for resource-heavy spin options (to avoid allocating e.g. 32 TB
/// of hash or spawning 1024 threads on a shared machine).
fn resource_cap(name: &str) -> Option<i64> {
    let n = name.to_ascii_lowercase();
    if n.contains("thread") {
        Some(4)
    } else if n.contains("hash") || n.contains("memory") || n.contains("cache") || n.contains("table size") {
        Some(64)
    } else {
        None
    }
}

impl Tester {
    fn pass(&mut self, name: &str, info: &str) {
        self.pass += 1;
        if info.is_empty() {
            outln!("PASS  {}", name);
        } else {
            outln!("PASS  {} — {}", name, info);
        }
    }

    fn fail(&mut self, name: &str, why: &str) {
        self.fail += 1;
        self.failed_names.push(name.to_string());
        outln!("FAIL  {} — {}", name, why);
    }

    fn warn(&mut self, name: &str, why: &str) {
        self.warn += 1;
        outln!("WARN  {} — {}", name, why);
    }

    fn eng(&mut self) -> &mut Engine {
        self.eng.as_mut().expect("engine running")
    }

    /// Kill the current process and start a fresh one (handshake + user options).
    fn restart(&mut self) -> Result<(), String> {
        if let Some(mut e) = self.eng.take() {
            e.kill();
        }
        let mut cfg = self.cfg.clone();
        cfg.options = self.user_opts.clone();
        let e = Engine::start(&cfg, Duration::from_secs(10), Duration::from_secs(10))?;
        self.eng = Some(e);
        Ok(())
    }

    fn ensure_alive(&mut self) -> bool {
        let alive = match self.eng.as_mut() {
            Some(e) => !e.dead && e.is_alive(),
            None => false,
        };
        if alive {
            return true;
        }
        match self.restart() {
            Ok(()) => {
                outln!("      (engine restarted)");
                true
            }
            Err(e) => {
                outln!("      (engine restart failed: {})", e);
                false
            }
        }
    }

    fn isready(&mut self, timeout: Duration) -> Result<Duration, String> {
        let t = Instant::now();
        self.eng().isready(timeout)?;
        Ok(t.elapsed())
    }

    /// After a failed search: try `stop` + `isready`; restart if the engine stays unresponsive.
    fn recover(&mut self) {
        if let Some(e) = self.eng.as_mut() {
            if !e.dead && e.is_alive() {
                let _ = e.send("stop");
                let _ = wait_bestmove(e, Instant::now() + Duration::from_millis(1000));
                if e.isready(Duration::from_secs(2)).is_ok() {
                    return;
                }
            }
        }
        let _ = self.ensure_alive_force();
    }

    fn ensure_alive_force(&mut self) -> bool {
        match self.restart() {
            Ok(()) => {
                outln!("      (engine restarted)");
                true
            }
            Err(e) => {
                outln!("      (engine restart failed: {})", e);
                false
            }
        }
    }

    /// Send position + go, wait for bestmove.
    fn search(&mut self, pos: &str, go: &str, timeout: Duration) -> Result<(SearchReply, Duration), String> {
        let e = self.eng();
        e.drain();
        e.send(&format!("{}\n{}", pos, go))?;
        let t0 = Instant::now();
        match wait_bestmove(e, t0 + timeout) {
            SearchOutcome::Reply(r) => {
                let el = r.at.saturating_duration_since(t0);
                Ok((r, el))
            }
            SearchOutcome::Timeout(_) => Err(format!("no bestmove within {} ms", ms(timeout))),
            SearchOutcome::Crashed(_) => {
                let st = e.exit_status().unwrap_or_else(|| "pipe closed".into());
                Err(format!("engine died during search ({})", st))
            }
            SearchOutcome::Aborted => Err("aborted".into()),
        }
    }

    /// Verify the reply is a legal move in `board`; returns its SAN.
    fn verify_legal(&mut self, board: &Board, r: &SearchReply) -> Result<String, String> {
        match san::parse_uci(board, &r.bestmove) {
            Some(m) => {
                if let Some(p) = &r.ponder {
                    let mut after = board.clone();
                    after.play_unchecked(m);
                    if san::has_legal_move(&after) && san::parse_uci(&after, p).is_none() {
                        self.warn("ponder move legality", &format!("bestmove {} ponder {}: ponder move is illegal", r.bestmove, p));
                    }
                }
                Ok(san::san(board, m))
            }
            None => match san::nonstandard_castle_hint(board, &r.bestmove) {
                Some(h) => Err(format!("illegal bestmove '{}' in {} ({})", r.bestmove, board, h)),
                None => Err(format!("illegal bestmove '{}' in {}", r.bestmove, board)),
            },
        }
    }

    /// One search check: legal bestmove expected; returns (elapsed, book move?).
    fn search_check(&mut self, name: &str, fen: Option<&str>, moves: &[&str], go: &str, timeout: Duration) -> Option<(Duration, bool)> {
        if !self.ensure_alive() {
            self.fail(name, "engine not running");
            return None;
        }
        let board = match board_of(fen, moves) {
            Ok(b) => b,
            Err(e) => {
                self.fail(name, &format!("internal: {}", e));
                return None;
            }
        };
        let pos = position_cmd(fen, moves);
        match self.search(&pos, go, timeout) {
            Ok((r, el)) => match self.verify_legal(&board, &r) {
                Ok(s) => {
                    let sc = r.score.map(|v| format!(" score {}", crate::pgn::fmt_score(v))).unwrap_or_default();
                    let bk = if r.book { " (book)" } else { "" };
                    self.pass(name, &format!("bestmove {} ({}) in {} ms{}{}", r.bestmove, s, ms(el), sc, bk));
                    Some((el, r.book))
                }
                Err(e) => {
                    self.fail(name, &e);
                    None
                }
            },
            Err(e) => {
                self.fail(name, &e);
                self.recover();
                None
            }
        }
    }

    fn test_option(&mut self, o: &UciOption) {
        let label = format!("option {}", o.describe());
        let mut tried: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        let mut warns: Vec<String> = Vec::new();
        // (value, tolerant): tolerant values only WARN on failure
        let mut values: Vec<(Option<String>, bool, String)> = Vec::new();
        match o.kind.as_str() {
            "spin" => match (o.min, o.max) {
                (Some(lo), Some(hi)) => {
                    let cap = resource_cap(&o.name);
                    values.push((Some(lo.to_string()), false, format!("{} (min)", lo)));
                    let hi_eff = cap.map(|c| hi.min(c.max(lo))).unwrap_or(hi);
                    if hi_eff != hi {
                        values.push((Some(hi_eff.to_string()), false, format!("{} (max capped from {})", hi_eff, hi)));
                    } else {
                        values.push((Some(hi.to_string()), false, format!("{} (max)", hi)));
                    }
                    if let Some(d) = &o.default {
                        values.push((Some(d.clone()), false, format!("{} (default)", d)));
                    }
                    values.push((Some((lo - 1).to_string()), false, format!("{} (below min)", lo - 1)));
                    if cap.is_none() {
                        values.push((Some((hi.saturating_add(1)).to_string()), false, format!("{} (above max)", hi.saturating_add(1))));
                    }
                }
                _ => {
                    self.warn(&label, "spin option without min/max");
                    if let Some(d) = &o.default {
                        values.push((Some(d.clone()), false, format!("{} (default)", d)));
                    }
                }
            },
            "check" => {
                values.push((Some("true".into()), false, "true".into()));
                values.push((Some("false".into()), false, "false".into()));
                if let Some(d) = &o.default {
                    if d != "true" && d != "false" {
                        warns.push(format!("default '{}' is not true/false", d));
                    }
                }
            }
            "combo" => {
                for v in &o.vars {
                    values.push((Some(v.clone()), false, v.clone()));
                }
                values.push((Some("NoSuchVariant".into()), false, "NoSuchVariant (invalid)".into()));
            }
            "button" => values.push((None, false, "(press)".into())),
            "string" => {
                let d = o.default.clone().unwrap_or_default();
                values.push((Some(d.clone()), false, format!("'{}' (default)", d)));
                values.push((
                    Some("/nonexistent/aggro_arena_ucitest/none".into()),
                    true,
                    "'/nonexistent/...' (missing path)".into(),
                ));
            }
            other => {
                self.warn(&label, &format!("unknown option type '{}'", other));
                return;
            }
        }
        for (val, tolerant, desc) in values {
            if !self.ensure_alive() {
                errors.push(format!("{}: engine not running", desc));
                break;
            }
            let cmd = match &val {
                None => format!("setoption name {}", o.name),
                Some(v) if v.is_empty() => format!("setoption name {} value", o.name),
                Some(v) => format!("setoption name {} value {}", o.name, v),
            };
            let r = self.eng().send(&cmd).and_then(|_| self.eng().isready(Duration::from_secs(10)));
            match r {
                Ok(()) => tried.push(desc),
                Err(e) => {
                    let died = !self.eng().is_alive();
                    let msg = format!("{}: {}{}", desc, e, if died { " [engine exited]" } else { "" });
                    if tolerant {
                        warns.push(msg);
                    } else {
                        errors.push(msg);
                    }
                    let _ = self.ensure_alive_force();
                }
            }
        }
        // restore: user value, else default
        if self.ensure_alive() {
            let user = self.user_opts.iter().find(|(k, _)| k.eq_ignore_ascii_case(&o.name)).map(|(_, v)| v.clone());
            let restore = user.or_else(|| o.default.clone());
            if o.kind != "button" {
                if let Some(v) = restore {
                    let cmd = if v.is_empty() { format!("setoption name {} value", o.name) } else { format!("setoption name {} value {}", o.name, v) };
                    let ok = self.eng().send(&cmd).and_then(|_| self.eng().isready(Duration::from_secs(10)));
                    if let Err(e) = ok {
                        errors.push(format!("restoring '{}': {}", v, e));
                        let _ = self.ensure_alive_force();
                    }
                }
            }
        }
        if !warns.is_empty() {
            self.warn(&label, &warns.join("; "));
        }
        if errors.is_empty() {
            self.pass(&label, &format!("still ready after {}", tried.join(", ")));
        } else {
            self.fail(&label, &errors.join("; "));
        }
    }

    /// A position without legal moves: expect `bestmove 0000` / `(none)` and no crash.
    fn no_move_check(&mut self, name: &str, fen: &str) {
        if !self.ensure_alive() {
            self.fail(name, "engine not running");
            return;
        }
        let board = Board::from_fen(fen, false).unwrap();
        if san::has_legal_move(&board) {
            self.fail(name, "internal: test position has legal moves");
            return;
        }
        match self.search(&position_cmd(Some(fen), &[]), "go movetime 500", Duration::from_secs(5)) {
            Ok((r, el)) => {
                let ok_str = r.bestmove == "0000" || r.bestmove == "(none)";
                let ready = self.eng().isready(Duration::from_secs(3));
                match (ok_str, ready) {
                    (true, Ok(())) => self.pass(name, &format!("bestmove {} in {} ms, still ready", r.bestmove, ms(el))),
                    (false, Ok(())) => self.fail(name, &format!("expected bestmove 0000/(none), got '{}'", r.bestmove)),
                    (_, Err(e)) => {
                        self.fail(name, &format!("bestmove '{}' but then not ready: {}", r.bestmove, e));
                        self.recover();
                    }
                }
            }
            Err(e) => {
                self.fail(name, &e);
                self.recover();
            }
        }
    }

    fn run(&mut self) -> i32 {
        // ---- 1. start + uci
        let mut e = match Engine::spawn(&self.cfg) {
            Ok(e) => e,
            Err(err) => {
                self.fail("start engine", &err);
                return self.finish();
            }
        };
        let t = Instant::now();
        match e.handshake(Duration::from_secs(10)) {
            Ok(()) => {
                let el = t.elapsed();
                self.eng = Some(e);
                self.pass("uci -> uciok", &format!("{} ms", ms(el)));
            }
            Err(err) => {
                self.fail("uci -> uciok", &err);
                return self.finish();
            }
        }
        let (idn, ida, opts) = {
            let e = self.eng();
            (e.id_name.clone(), e.id_author.clone(), e.options.clone())
        };
        match idn {
            Some(n) => self.pass("id name", &format!("'{}' (author '{}')", n, ida.unwrap_or_default())),
            None => self.fail("id name", "no 'id name' line before uciok"),
        }
        outln!("      advertised options ({}):", opts.len());
        for o in &opts {
            outln!("        - {}", o.describe());
        }

        // ---- 2. isready
        match self.isready(Duration::from_secs(5)) {
            Ok(d) => self.pass("isready -> readyok", &format!("{} ms", ms(d))),
            Err(err) => {
                self.fail("isready -> readyok", &err);
                self.recover();
            }
        }

        // ---- user options
        if !self.user_opts.is_empty() {
            let uo = self.user_opts.clone();
            let mut errs = Vec::new();
            for (k, v) in &uo {
                if !opts.iter().any(|o| o.name.eq_ignore_ascii_case(k)) {
                    self.warn("user options", &format!("option '{}' is not advertised", k));
                }
                if let Err(err) = self.eng().set_option(k, v) {
                    errs.push(err);
                }
            }
            match self.isready(Duration::from_secs(10)) {
                Ok(_) if errs.is_empty() => {
                    let s: Vec<String> = uo.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
                    self.pass("user options applied", &s.join(", "))
                }
                Ok(_) => self.fail("user options applied", &errs.join("; ")),
                Err(err) => {
                    self.fail("user options applied", &err);
                    self.recover();
                }
            }
        }

        // ---- 3. every advertised option
        for o in &opts {
            self.test_option(o);
        }
        // Option round-trips can leave an engine in a degraded state (e.g. Stockfish 16
        // keeps a zeroed network after EvalFile=<missing path> + restore), so the
        // search checks below run on a fresh process.
        if !opts.is_empty() {
            match self.restart() {
                Ok(()) => outln!("      (fresh engine process for the search checks)"),
                Err(e) => self.fail("restart after option tests", &e),
            }
        }

        // ---- 4. ucinewgame
        if self.ensure_alive() {
            let r = self.eng().send("ucinewgame").and_then(|_| self.eng().isready(Duration::from_secs(10)));
            match r {
                Ok(()) => self.pass("ucinewgame + isready", ""),
                Err(err) => {
                    self.fail("ucinewgame + isready", &err);
                    self.recover();
                }
            }
        }

        // ---- 5. startpos depth 6
        self.search_check("position startpos + go depth 6", None, &[], "go depth 6", Duration::from_secs(30));

        // ---- 6. FEN positions + movetime 200
        let fens: &[(&str, &str, &[&str])] = &[
            ("castling moves in move list", "r3k2r/pppq1ppp/2npbn2/2b1p3/2B1P3/2NPBN2/PPPQ1PPP/R3K2R w KQkq - 0 1", &["e1g1", "e8c8"]),
            ("castling available", "r3k2r/pppq1ppp/2npbn2/2b1p3/2B1P3/2NPBN2/PPPQ1PPP/R3K2R w KQkq - 0 1", &[]),
            ("en passant available", "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3", &[]),
            ("en passant in move list", "rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3", &["e5f6", "g8f6"]),
            ("promotion available", "1r5k/P7/8/8/8/8/7p/4K3 w - - 0 1", &[]),
            ("promotions in move list (incl. underpromotion)", "8/P7/8/8/8/8/2k4p/4K3 w - - 0 1", &["a7a8q", "h2h1n"]),
            ("side to move in check", "rnbqkbnr/ppp2ppp/8/1B1pp3/4P3/8/PPPP1PPP/RNBQK1NR b KQkq - 1 3", &[]),
            ("black to move, middlegame", MIDGAME_B, &[]),
        ];
        let think = human_think_on(&self.user_opts);
        for (name, fen, moves) in fens {
            let label = format!("fen + go movetime 200: {}", name);
            let tol = if think { Duration::from_secs(60) } else { Duration::from_millis(5000) };
            if let Some((el, _)) = self.search_check(&label, Some(fen), moves, "go movetime 200", tol) {
                if !think && el > Duration::from_millis(200 + 300) {
                    self.warn(&label, &format!("movetime 200 overrun: {} ms", ms(el)));
                }
                if think {
                    self.warn(&label, "human think time is on: reply includes the deliberate pause");
                }
            }
        }

        // ---- 7. clock handling
        // (name, fen, go, budget = remaining clock, warn if more than half is used)
        let clock_tests: &[(&str, &str, &str, u64, bool)] = &[
            ("white clock: wtime 3000 winc 30", MIDGAME_W, "go wtime 3000 btime 3000 winc 30 binc 30", 3000, true),
            ("black clock: btime 3000 (wtime 100000)", MIDGAME_B, "go wtime 100000 btime 3000 winc 30 binc 30", 3000, true),
            ("low time: wtime 300, no increment", MIDGAME_W, "go wtime 300 btime 300", 300, true),
            // last move before the time control: using most of the clock is legitimate
            ("movestogo 1: wtime 1000", MIDGAME_W, "go wtime 1000 btime 1000 movestogo 1", 1000, false),
        ];
        for (name, fen, go, budget, warn_half) in clock_tests {
            let label = format!("time management, {}", name);
            if let Some((el, book)) = self.search_check(&label, Some(fen), &[], go, Duration::from_millis(budget + 5000)) {
                if book {
                    self.warn(&label, "answered from the opening book; time management not exercised");
                }
                if el > Duration::from_millis(*budget) {
                    self.fail(&label, &format!("used {} ms of a {} ms budget (would lose on time)", ms(el), budget));
                } else if *warn_half && el > Duration::from_millis(budget / 2) {
                    self.warn(&label, &format!("used {} ms of {} ms (more than half the remaining time)", ms(el), budget));
                }
            }
        }

        // ---- 8. go infinite + stop
        let label = "go infinite, stop after 500 ms -> bestmove within 1 s";
        if self.ensure_alive() {
            let board = Board::from_fen(COMPLEX, false).unwrap();
            let e = self.eng();
            e.drain();
            let sent = e.send(&format!("{}\ngo infinite", position_cmd(Some(COMPLEX), &[])));
            let t0 = Instant::now();
            let early = match sent {
                Err(err) => Err(err),
                Ok(()) => match wait_bestmove(e, t0 + Duration::from_millis(500)) {
                    SearchOutcome::Timeout(_) => Ok(None),
                    SearchOutcome::Reply(r) => Ok(Some(r)),
                    SearchOutcome::Crashed(_) => Err("engine died during go infinite".to_string()),
                    SearchOutcome::Aborted => Err("aborted".into()),
                },
            };
            match early {
                Err(err) => {
                    self.fail(label, &err);
                    self.recover();
                }
                Ok(Some(r)) => {
                    self.fail(label, &format!("bestmove {} sent after {} ms, before stop (go infinite must wait for stop)", r.bestmove, ms(r.at - t0)));
                    self.recover();
                }
                Ok(None) => {
                    let e = self.eng();
                    let _ = e.send("stop");
                    let ts = Instant::now();
                    match wait_bestmove(e, ts + Duration::from_millis(1000)) {
                        SearchOutcome::Reply(r) => {
                            let lat = r.at.saturating_duration_since(ts);
                            match self.verify_legal(&board, &r) {
                                Ok(s) => self.pass(label, &format!("bestmove {} ({}) {} ms after stop", r.bestmove, s, ms(lat))),
                                Err(err) => self.fail(label, &err),
                            }
                        }
                        SearchOutcome::Timeout(_) => {
                            self.fail(label, "no bestmove within 1000 ms of stop");
                            self.recover();
                        }
                        _ => {
                            self.fail(label, "engine died after stop");
                            self.recover();
                        }
                    }
                }
            }
        }

        // ---- 9. stop without a search
        let label = "stop with no search running";
        if self.ensure_alive() {
            let _ = self.eng().isready(Duration::from_secs(2));
            let e = self.eng();
            e.drain();
            let r = e.send("stop").and_then(|_| e.send("isready"));
            match r.and_then(|_| e.wait_for(|l| l == "readyok", Duration::from_secs(3)).map_err(|w| w.describe("readyok"))) {
                Ok(lines) => {
                    let spurious: Vec<&str> = lines.iter().map(|l| l.text.as_str()).filter(|t| t.trim_start().starts_with("bestmove")).collect();
                    if spurious.is_empty() {
                        self.pass(label, "ignored, still ready");
                    } else {
                        self.pass(label, "no crash, still ready");
                        self.warn(label, &format!("engine printed '{}' although no search was running", spurious[0]));
                    }
                }
                Err(err) => {
                    self.fail(label, &err);
                    self.recover();
                }
            }
        }

        // ---- 10. mated / stalemate / single legal move
        self.no_move_check("checkmated side to move -> bestmove 0000/(none)", "rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3");
        self.no_move_check("stalemate -> bestmove 0000/(none)", "7k/5Q2/6K1/8/8/8/8/8 b - - 0 1");
        {
            let fen = "7k/8/8/8/8/8/6R1/6RK b - - 0 1";
            let label = "exactly one legal move";
            let b = Board::from_fen(fen, false).unwrap();
            let legal = san::legal_moves(&b);
            if legal.len() != 1 {
                self.fail(label, "internal: test position does not have exactly one legal move");
            } else if let Some((el, _)) = self.search_check(label, Some(fen), &[], "go wtime 10000 btime 10000", Duration::from_secs(15)) {
                if el > Duration::from_millis(1000) {
                    self.warn(label, &format!("took {} ms for a forced move", ms(el)));
                }
            }
        }

        // ---- 11. 50 x ucinewgame/position/go nodes 2000
        {
            let label = "50 x ucinewgame/position/go nodes 2000";
            let mut seed = 0x2545_F491_4F6C_DD1Du64;
            let mut errs: Vec<String> = Vec::new();
            let t0 = Instant::now();
            let mut max_el = Duration::ZERO;
            for cyc in 0..50 {
                if !self.ensure_alive() {
                    errs.push(format!("cycle {}: engine not running", cyc));
                    break;
                }
                // pseudo-random legal position reached from the start position
                let mut b = Board::default();
                let mut mv: Vec<String> = Vec::new();
                let len = (cyc * 7) % 41;
                for _ in 0..len {
                    let legal = san::legal_moves(&b);
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    let m = legal[(seed % legal.len() as u64) as usize];
                    let mut nb = b.clone();
                    nb.play_unchecked(m);
                    if !san::has_legal_move(&nb) {
                        break;
                    }
                    mv.push(san::uci_string(&b, m));
                    b = nb;
                }
                let refs: Vec<&str> = mv.iter().map(|s| s.as_str()).collect();
                let r = self.eng().send("ucinewgame").and_then(|_| self.eng().isready(Duration::from_secs(10)));
                if let Err(e) = r {
                    errs.push(format!("cycle {}: {}", cyc, e));
                    self.recover();
                    continue;
                }
                match self.search(&position_cmd(None, &refs), "go nodes 2000", Duration::from_secs(10)) {
                    Ok((r, el)) => {
                        max_el = max_el.max(el);
                        if let Err(e) = self.verify_legal(&b, &r) {
                            errs.push(format!("cycle {}: {}", cyc, e));
                        }
                    }
                    Err(e) => {
                        errs.push(format!("cycle {}: {}", cyc, e));
                        self.recover();
                    }
                }
            }
            if errs.is_empty() {
                self.pass(label, &format!("all legal, total {} ms, slowest {} ms", ms(t0.elapsed()), ms(max_el)));
            } else {
                let n = errs.len();
                errs.truncate(5);
                self.fail(label, &format!("{} failures: {}", n, errs.join("; ")));
            }
        }

        // ---- 12. garbage input
        {
            let label = "garbage input lines are ignored";
            if self.ensure_alive() {
                let garbage = [
                    "hello engine",
                    "",
                    "   ",
                    "xyzzy 1 2 3",
                    "setoption name ThisOptionDoesNotExist value 42",
                    "isready_not",
                    "\u{00e9}\u{00e8} unicode garbage",
                ];
                let mut res = Ok(());
                for g in garbage {
                    if let Err(e) = self.eng().send(g) {
                        res = Err(e);
                        break;
                    }
                }
                let res = res.and_then(|_| self.eng().isready(Duration::from_secs(5)));
                match res {
                    Ok(()) => {
                        // PASS/FAIL printed by search_check
                        let _ = self.search_check(label, None, &["e2e4"], "go movetime 100", Duration::from_secs(5));
                    }
                    Err(e) => {
                        self.fail(label, &e);
                        self.recover();
                    }
                }
            }
        }

        // ---- 13. go immediately followed by quit
        {
            let label = "go immediately followed by quit -> exit within 2 s";
            if self.ensure_alive() {
                let e = self.eng();
                let r = e.send(&format!("{}\ngo\nquit", position_cmd(Some(COMPLEX), &[])));
                let t0 = Instant::now();
                match r {
                    Err(err) => self.fail(label, &err),
                    Ok(()) => {
                        if e.wait_exit(Duration::from_secs(2)) {
                            let el = t0.elapsed();
                            let st = e.exit_status().unwrap_or_else(|| "exited".into());
                            self.pass(label, &format!("exited after {} ms ({})", ms(el), st));
                        } else {
                            self.fail(label, "process still running 2 s after quit (killed)");
                        }
                    }
                }
                if let Some(mut e) = self.eng.take() {
                    e.kill();
                }
            }
        }
        self.finish()
    }

    fn finish(&mut self) -> i32 {
        if let Some(mut e) = self.eng.take() {
            e.quit(Duration::from_millis(500));
        }
        outln!();
        outln!("ucitest summary: {} passed, {} failed, {} warnings", self.pass, self.fail, self.warn);
        if self.fail > 0 {
            outln!("failed checks:");
            for n in &self.failed_names {
                outln!("  - {}", n);
            }
            1
        } else {
            0
        }
    }
}

pub fn cmd(args: &[String]) -> i32 {
    let usage = "usage: aggro_arena ucitest <engine-path> [--option \"Name=Value\"]... [--cwd DIR] [--arg ARG]...";
    let mut path: Option<String> = None;
    let mut opts: Vec<(String, String)> = Vec::new();
    let mut cwd: Option<std::path::PathBuf> = None;
    let mut eargs: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--option" | "-o" => {
                let v = match args.get(i + 1) {
                    Some(v) => v,
                    None => {
                        errln!("{}", usage);
                        return 2;
                    }
                };
                match v.split_once('=') {
                    Some((k, val)) => opts.push((k.trim().to_string(), val.trim().to_string())),
                    None => {
                        errln!("--option needs Name=Value");
                        return 2;
                    }
                }
                i += 2;
            }
            "--cwd" => {
                cwd = args.get(i + 1).map(std::path::PathBuf::from);
                i += 2;
            }
            "--arg" => {
                if let Some(a) = args.get(i + 1) {
                    eargs.push(a.clone());
                }
                i += 2;
            }
            "-h" | "--help" => {
                outln!("{}", usage);
                return 0;
            }
            a => {
                if path.is_some() {
                    errln!("{}", usage);
                    return 2;
                }
                path = Some(a.to_string());
                i += 1;
            }
        }
    }
    let path = match path {
        Some(p) => p,
        None => {
            errln!("{}", usage);
            return 2;
        }
    };
    // A relative path with a '/' would be resolved against --cwd by the OS (the child's
    // working directory); resolve it against the invocation directory instead.
    let path = if path.contains('/') && !std::path::Path::new(&path).is_absolute() {
        std::env::current_dir().map(|d| d.join(&path).to_string_lossy().into_owned()).unwrap_or(path)
    } else {
        path
    };
    let mut cfg = EngineConfig::new("engine", &path);
    cfg.cwd = cwd;
    cfg.args = eargs;
    outln!("ucitest: {}", path);
    let mut t = Tester { cfg, user_opts: opts, eng: None, pass: 0, fail: 0, warn: 0, failed_names: Vec::new() };
    t.run()
}
