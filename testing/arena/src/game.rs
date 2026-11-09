//! One game between two UCI engines: rules, termination, clocks, and the
//! per-game record (JSONL serialisation + reader).

use std::time::{Duration, Instant};

use cozy_chess::{Board, Color, Move};

use crate::engine::{last_line, wait_bestmove, Engine, SearchOutcome};
use crate::json::Json;
use crate::san;
use crate::sys;

// ---------------------------------------------------------------- time control

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tc {
    /// Fischer clock: base + increment per move (milliseconds).
    Clock { base_ms: u64, inc_ms: u64 },
    Nodes(u64),
    Movetime(u64),
    Depth(u32),
}

/// Hard per-move timeout for nodes/depth/movetime TCs.
pub const FIXED_TC_TIMEOUT: Duration = Duration::from_secs(60);

fn fmt_secs(ms: u64) -> String {
    if ms.is_multiple_of(1000) {
        (ms / 1000).to_string()
    } else {
        let s = format!("{:.3}", ms as f64 / 1000.0);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

impl Tc {
    pub fn parse(s: &str) -> Result<Tc, String> {
        let s = s.trim();
        let num = |v: &str, what: &str| -> Result<u64, String> {
            v.trim().parse::<u64>().map_err(|_| format!("bad {} value '{}'", what, v))
        };
        if let Some(v) = s.strip_prefix("nodes=") {
            let n = num(v, "nodes")?;
            return if n == 0 { Err("nodes must be > 0".into()) } else { Ok(Tc::Nodes(n)) };
        }
        if let Some(v) = s.strip_prefix("movetime=") {
            let n = num(v, "movetime")?;
            return if n == 0 { Err("movetime must be > 0".into()) } else { Ok(Tc::Movetime(n)) };
        }
        if let Some(v) = s.strip_prefix("depth=") {
            let n = num(v, "depth")?;
            return if n == 0 || n > 1000 { Err("depth must be 1..1000".into()) } else { Ok(Tc::Depth(n as u32)) };
        }
        let (b, i) = match s.split_once('+') {
            Some((b, i)) => (b, i),
            None => (s, "0"),
        };
        let secs = |v: &str| -> Result<u64, String> {
            let f: f64 = v.trim().parse().map_err(|_| format!("bad time control '{}'", s))?;
            if !(f.is_finite()) || f < 0.0 {
                return Err(format!("bad time control '{}'", s));
            }
            Ok((f * 1000.0).round() as u64)
        };
        let base_ms = secs(b)?;
        let inc_ms = secs(i)?;
        if base_ms == 0 && inc_ms == 0 {
            return Err(format!("time control '{}' has no time", s));
        }
        Ok(Tc::Clock { base_ms, inc_ms })
    }

    /// Canonical text ("10+0.1", "nodes=20000", "movetime=500", "depth=8").
    pub fn label(&self) -> String {
        match *self {
            Tc::Clock { base_ms, inc_ms } => format!("{}+{}", fmt_secs(base_ms), fmt_secs(inc_ms)),
            Tc::Nodes(n) => format!("nodes={}", n),
            Tc::Movetime(n) => format!("movetime={}", n),
            Tc::Depth(n) => format!("depth={}", n),
        }
    }
}

// ---------------------------------------------------------------- results

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameResult {
    WhiteWins,
    BlackWins,
    Draw,
}

impl GameResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            GameResult::WhiteWins => "1-0",
            GameResult::BlackWins => "0-1",
            GameResult::Draw => "1/2-1/2",
        }
    }

    pub fn parse(s: &str) -> Option<GameResult> {
        match s {
            "1-0" => Some(GameResult::WhiteWins),
            "0-1" => Some(GameResult::BlackWins),
            "1/2-1/2" => Some(GameResult::Draw),
            _ => None,
        }
    }

    pub fn win_for(c: Color) -> GameResult {
        if c == Color::White {
            GameResult::WhiteWins
        } else {
            GameResult::BlackWins
        }
    }

    /// Score (1, 0.5, 0) for the given colour.
    pub fn score_for(&self, c: Color) -> f64 {
        match (self, c) {
            (GameResult::Draw, _) => 0.5,
            (GameResult::WhiteWins, Color::White) | (GameResult::BlackWins, Color::Black) => 1.0,
            _ => 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Termination {
    Checkmate,
    Stalemate,
    Insufficient,
    Fifty,
    Threefold,
    Maxplies,
    Resign,
    Time,
    Illegal,
    Crash,
}

impl Termination {
    pub const ALL: [Termination; 10] = [
        Termination::Checkmate,
        Termination::Stalemate,
        Termination::Insufficient,
        Termination::Fifty,
        Termination::Threefold,
        Termination::Maxplies,
        Termination::Resign,
        Termination::Time,
        Termination::Illegal,
        Termination::Crash,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Termination::Checkmate => "checkmate",
            Termination::Stalemate => "stalemate",
            Termination::Insufficient => "insufficient",
            Termination::Fifty => "fifty",
            Termination::Threefold => "threefold",
            Termination::Maxplies => "maxplies",
            Termination::Resign => "resign",
            Termination::Time => "time",
            Termination::Illegal => "illegal",
            Termination::Crash => "crash",
        }
    }

    pub fn parse(s: &str) -> Option<Termination> {
        Termination::ALL.iter().copied().find(|t| t.as_str() == s)
    }

    /// Terminations caused by an engine failure rather than by the game.
    pub fn is_forfeit(&self) -> bool {
        matches!(self, Termination::Time | Termination::Illegal | Termination::Crash)
    }
}

fn color_name(c: Color) -> &'static str {
    if c == Color::White {
        "white"
    } else {
        "black"
    }
}

fn parse_color(s: &str) -> Option<Color> {
    match s {
        "white" => Some(Color::White),
        "black" => Some(Color::Black),
        _ => None,
    }
}

// ---------------------------------------------------------------- record

/// One finished game (one JSONL line). Public so other subcommands (e.g. a
/// future `analyze`) can read match output with [`read_jsonl`].
#[derive(Debug, Clone, PartialEq)]
pub struct GameRecord {
    pub id: u64,
    pub white: String,
    pub black: String,
    /// Index of the `pair` line in the config (0-based).
    pub pairing: usize,
    /// Index of the opening pair within its pairing; both games of a colour-reversed
    /// pair share (pairing, game_pair).
    pub game_pair: usize,
    /// 0-based line index in the openings file, -1 = start position.
    pub opening_index: i64,
    pub opening_plies: usize,
    /// UCI moves from the start position (castling as e1g1).
    pub moves: Vec<String>,
    pub san: Vec<String>,
    /// Mover's score in cp (mates ±(100000-n)); None for opening/book/no info.
    pub scores: Vec<Option<i32>>,
    pub depths: Vec<Option<u32>>,
    pub times_ms: Vec<u64>,
    pub book: Vec<bool>,
    pub result: GameResult,
    pub termination: Termination,
    pub detail: String,
    pub tc: String,
    /// Side whose failure ended the game (time/illegal/crash), if any.
    pub forfeit_side: Option<Color>,
    /// Remaining clock (ms) of white/black at the end (clock TCs only).
    pub clock_end_ms: Option<(i64, i64)>,
    /// UTC timestamp when the game finished.
    pub date: String,
}

impl GameRecord {
    pub fn to_json(&self) -> Json {
        let mut j = Json::obj()
            .with("id", self.id)
            .with("white", &self.white)
            .with("black", &self.black)
            .with("pairing", self.pairing)
            .with("game_pair", self.game_pair)
            .with("opening_index", self.opening_index)
            .with("opening_plies", self.opening_plies)
            .with("moves", self.moves.clone())
            .with("san", self.san.clone())
            .with("scores", self.scores.iter().map(|s| s.map(|x| x as i64)).collect::<Vec<_>>())
            .with("depths", self.depths.iter().map(|d| d.map(|x| x as i64)).collect::<Vec<_>>())
            .with("times_ms", self.times_ms.clone())
            .with("book", self.book.clone())
            .with("result", self.result.as_str())
            .with("termination", self.termination.as_str())
            .with("detail", &self.detail)
            .with("tc", &self.tc)
            .with("forfeit_side", self.forfeit_side.map(color_name));
        match self.clock_end_ms {
            Some((w, b)) => j.set("clock_end_ms", vec![w, b]),
            None => j.set("clock_end_ms", Json::Null),
        }
        j.set("date", &self.date);
        j
    }

    pub fn from_json(j: &Json) -> Result<GameRecord, String> {
        fn req<'a>(j: &'a Json, k: &str) -> Result<&'a Json, String> {
            j.get(k).ok_or_else(|| format!("missing field '{}'", k))
        }
        fn s(j: &Json, k: &str) -> Result<String, String> {
            req(j, k)?.as_str().map(|x| x.to_string()).ok_or_else(|| format!("field '{}' is not a string", k))
        }
        fn int(j: &Json, k: &str) -> Result<i64, String> {
            req(j, k)?.as_i64().ok_or_else(|| format!("field '{}' is not an integer", k))
        }
        fn arr<'a>(j: &'a Json, k: &str) -> Result<&'a [Json], String> {
            req(j, k)?.as_array().ok_or_else(|| format!("field '{}' is not an array", k))
        }
        let strs = |k: &str| -> Result<Vec<String>, String> {
            arr(j, k)?.iter().map(|v| v.as_str().map(|x| x.to_string()).ok_or_else(|| format!("bad entry in '{}'", k))).collect()
        };
        let opt_ints = |k: &str| -> Result<Vec<Option<i64>>, String> {
            arr(j, k)?
                .iter()
                .map(|v| if v.is_null() { Ok(None) } else { v.as_i64().map(Some).ok_or_else(|| format!("bad entry in '{}'", k)) })
                .collect()
        };
        let result = GameResult::parse(&s(j, "result")?).ok_or("bad result")?;
        let termination = Termination::parse(&s(j, "termination")?).ok_or("bad termination")?;
        let moves = strs("moves")?;
        let rec = GameRecord {
            id: int(j, "id")? as u64,
            white: s(j, "white")?,
            black: s(j, "black")?,
            pairing: j.get("pairing").and_then(|v| v.as_i64()).unwrap_or(0) as usize,
            game_pair: j.get("game_pair").and_then(|v| v.as_i64()).unwrap_or(0) as usize,
            opening_index: int(j, "opening_index")?,
            opening_plies: int(j, "opening_plies")? as usize,
            san: strs("san")?,
            scores: opt_ints("scores")?.into_iter().map(|v| v.map(|x| x as i32)).collect(),
            depths: opt_ints("depths")?.into_iter().map(|v| v.map(|x| x as u32)).collect(),
            times_ms: opt_ints("times_ms")?.into_iter().map(|v| v.unwrap_or(0) as u64).collect(),
            book: arr(j, "book")?.iter().map(|v| v.as_bool().unwrap_or(false)).collect(),
            result,
            termination,
            detail: j.get("detail").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            tc: j.get("tc").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            forfeit_side: j.get("forfeit_side").and_then(|v| v.as_str()).and_then(parse_color),
            clock_end_ms: j.get("clock_end_ms").and_then(|v| v.as_array()).and_then(|a| {
                if a.len() == 2 {
                    Some((a[0].as_i64()?, a[1].as_i64()?))
                } else {
                    None
                }
            }),
            date: j.get("date").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            moves,
        };
        let n = rec.moves.len();
        if rec.san.len() != n || rec.scores.len() != n || rec.depths.len() != n || rec.times_ms.len() != n || rec.book.len() != n {
            return Err("per-move arrays have different lengths".into());
        }
        Ok(rec)
    }
}

/// Read a JSONL file written by `match` (blank lines ignored).
pub fn read_jsonl(path: &std::path::Path) -> Result<Vec<GameRecord>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let j = Json::parse(line).map_err(|e| format!("{}:{}: {}", path.display(), i + 1, e))?;
        out.push(GameRecord::from_json(&j).map_err(|e| format!("{}:{}: {}", path.display(), i + 1, e))?);
    }
    Ok(out)
}

// ---------------------------------------------------------------- playing

#[derive(Debug, Clone, Copy)]
pub struct Resign {
    pub cp: i32,
    pub moves: u32,
}

#[derive(Debug, Clone)]
pub struct GameSettings {
    pub tc: Tc,
    pub maxplies: usize,
    pub timemargin_ms: u64,
    pub resign: Option<Resign>,
}

#[derive(Debug, Clone)]
pub struct GameSpec {
    pub id: u64,
    pub pairing: usize,
    pub game_pair: usize,
    /// true if the pairing's first engine (engA) has white in this game.
    pub a_is_white: bool,
    pub white: String,
    pub black: String,
    pub opening_index: i64,
    pub opening: Vec<String>,
}

struct Ongoing {
    rec: GameRecord,
    board: Board,
    /// repetition keys since the last irreversible move (incl. current position)
    hist: Vec<u64>,
    position_cmd: String,
}

impl Ongoing {
    fn new(spec: &GameSpec, tc: &Tc) -> Ongoing {
        let board = Board::default();
        let hist = vec![san::repetition_key(&board)];
        Ongoing {
            rec: GameRecord {
                id: spec.id,
                white: spec.white.clone(),
                black: spec.black.clone(),
                pairing: spec.pairing,
                game_pair: spec.game_pair,
                opening_index: spec.opening_index,
                opening_plies: 0,
                moves: Vec::new(),
                san: Vec::new(),
                scores: Vec::new(),
                depths: Vec::new(),
                times_ms: Vec::new(),
                book: Vec::new(),
                result: GameResult::Draw,
                termination: Termination::Maxplies,
                detail: String::new(),
                tc: tc.label(),
                forfeit_side: None,
                clock_end_ms: None,
                date: String::new(),
            },
            board,
            hist,
            position_cmd: String::from("position startpos"),
        }
    }

    fn push(&mut self, mv: Move, score: Option<i32>, depth: Option<u32>, time_ms: u64, book: bool) {
        let uci = san::uci_string(&self.board, mv);
        let s = san::san(&self.board, mv);
        if self.rec.moves.is_empty() {
            self.position_cmd.push_str(" moves");
        }
        self.position_cmd.push(' ');
        self.position_cmd.push_str(&uci);
        self.rec.moves.push(uci);
        self.rec.san.push(s);
        self.rec.scores.push(score);
        self.rec.depths.push(depth);
        self.rec.times_ms.push(time_ms);
        self.rec.book.push(book);
        self.board.play_unchecked(mv);
        if self.board.halfmove_clock() == 0 {
            self.hist.clear();
        }
        self.hist.push(san::repetition_key(&self.board));
    }

    /// Rule-based termination after a move (order as specified).
    fn check_end(&self, maxplies: usize) -> Option<(GameResult, Termination, String)> {
        let b = &self.board;
        let stm = b.side_to_move();
        if !san::has_legal_move(b) {
            if !b.checkers().is_empty() {
                return Some((GameResult::win_for(!stm), Termination::Checkmate, format!("{} mates", color_name(!stm))));
            }
            return Some((GameResult::Draw, Termination::Stalemate, format!("{} is stalemated", color_name(stm))));
        }
        if san::insufficient_material(b) {
            return Some((GameResult::Draw, Termination::Insufficient, "insufficient mating material".into()));
        }
        if b.halfmove_clock() >= 100 {
            return Some((GameResult::Draw, Termination::Fifty, "50-move rule".into()));
        }
        if let Some(&last) = self.hist.last() {
            if self.hist.iter().filter(|&&h| h == last).count() >= 3 {
                return Some((GameResult::Draw, Termination::Threefold, "threefold repetition".into()));
            }
        }
        if self.rec.moves.len() >= maxplies {
            return Some((GameResult::Draw, Termination::Maxplies, format!("adjudicated draw after {} plies", maxplies)));
        }
        None
    }

    fn finish(mut self, result: GameResult, term: Termination, detail: String, forfeit: Option<Color>) -> GameRecord {
        self.rec.result = result;
        self.rec.termination = term;
        self.rec.detail = detail;
        self.rec.forfeit_side = forfeit;
        self.rec.date = sys::now_iso_utc();
        self.rec
    }
}

/// Result of a forfeit by `loser`: loss, or a draw for time forfeits when the
/// opponent has only a bare king or king + one minor.
fn forfeit_result(board: &Board, loser: Color, term: Termination) -> GameResult {
    if term == Termination::Time && san::cannot_mate(board, !loser) {
        GameResult::Draw
    } else {
        GameResult::win_for(!loser)
    }
}

/// Record for a game that could not be played because an engine failed to
/// start / become ready (the opening is still replayed for consistency).
pub fn forfeit_record(spec: &GameSpec, tc: &Tc, loser: Color, detail: String) -> GameRecord {
    let mut g = Ongoing::new(spec, tc);
    for u in &spec.opening {
        match san::parse_uci(&g.board, u) {
            Some(mv) => g.push(mv, None, None, 0, false),
            None => break,
        }
    }
    g.rec.opening_plies = g.rec.moves.len();
    g.finish(GameResult::win_for(!loser), Termination::Crash, detail, Some(loser))
}

/// Play one game. `white`/`black` must be ready (after `ucinewgame` + `isready`).
/// Engines that crash or are killed are flagged `dead`. Returns None if the
/// match was interrupted (stop requested) — such games are not recorded.
pub fn play_game(spec: &GameSpec, white: &mut Engine, black: &mut Engine, settings: &GameSettings) -> Option<GameRecord> {
    let mut g = Ongoing::new(spec, &settings.tc);
    let names = [spec.white.as_str(), spec.black.as_str()];
    let idx = |c: Color| if c == Color::White { 0usize } else { 1 };

    // ---- opening moves from the suite
    for u in &spec.opening {
        let mv = match san::parse_uci(&g.board, u) {
            Some(m) => m,
            None => break, // validated when loading; defensive
        };
        g.push(mv, None, None, 0, false);
        g.rec.opening_plies = g.rec.moves.len();
        if let Some((r, t, d)) = g.check_end(settings.maxplies) {
            return Some(g.finish(r, t, d, None));
        }
    }
    g.rec.opening_plies = g.rec.moves.len();

    let margin_us = settings.timemargin_ms as i64 * 1000;
    let (mut clock_us, inc_us) = match settings.tc {
        Tc::Clock { base_ms, inc_ms } => ([base_ms as i64 * 1000; 2], inc_ms as i64 * 1000),
        _ => ([0; 2], 0),
    };
    let is_clock = matches!(settings.tc, Tc::Clock { .. });
    let mut resign_sign = 0i32;
    let mut resign_count = 0u32;

    loop {
        let side = g.board.side_to_move();
        let si = idx(side);
        let who = format!("{} ({})", color_name(side), names[si]);
        let eng: &mut Engine = if si == 0 { &mut *white } else { &mut *black };

        let go = match settings.tc {
            Tc::Clock { .. } => format!(
                "go wtime {} btime {} winc {} binc {}",
                (clock_us[0] / 1000).max(1),
                (clock_us[1] / 1000).max(1),
                inc_us / 1000,
                inc_us / 1000
            ),
            Tc::Nodes(n) => format!("go nodes {}", n),
            Tc::Movetime(m) => format!("go movetime {}", m),
            Tc::Depth(d) => format!("go depth {}", d),
        };
        let stale = eng.drain();
        let _ = stale; // anything left over from a previous command is irrelevant
        let cmd = format!("{}\n{}", g.position_cmd, go);
        if let Err(e) = eng.send(&cmd) {
            eng.kill();
            if is_clock {
                g.rec.clock_end_ms = Some((clock_us[0] / 1000, clock_us[1] / 1000));
            }
            let r = GameResult::win_for(!side);
            return Some(g.finish(r, Termination::Crash, format!("{} crashed: {}", who, e), Some(side)));
        }
        let go_time = Instant::now();
        let deadline = match settings.tc {
            Tc::Clock { .. } => {
                go_time + Duration::from_micros((clock_us[si].max(0) + margin_us) as u64) + Duration::from_millis(2000)
            }
            Tc::Movetime(m) => go_time + FIXED_TC_TIMEOUT.max(Duration::from_millis(m + 10_000)),
            _ => go_time + FIXED_TC_TIMEOUT,
        };

        let reply = match wait_bestmove(eng, deadline) {
            SearchOutcome::Aborted => return None,
            SearchOutcome::Timeout(tail) => {
                eng.kill();
                let waited = go_time.elapsed().as_millis();
                if is_clock {
                    clock_us[si] -= waited as i64 * 1000;
                    g.rec.clock_end_ms = Some((clock_us[0] / 1000, clock_us[1] / 1000));
                }
                let r = forfeit_result(&g.board, side, Termination::Time);
                let d = format!("{} sent no bestmove within {} ms (killed; {})", who, waited, last_line(&tail));
                return Some(g.finish(r, Termination::Time, d, Some(side)));
            }
            SearchOutcome::Crashed(tail) => {
                // give the OS a moment to report the exit status
                eng.wait_exit(Duration::from_millis(200));
                let status = eng.exit_status().unwrap_or_else(|| "stdout closed".into());
                eng.kill();
                if is_clock {
                    g.rec.clock_end_ms = Some((clock_us[0] / 1000, clock_us[1] / 1000));
                }
                let d = format!("{} crashed ({}; {})", who, status, last_line(&tail));
                return Some(g.finish(GameResult::win_for(!side), Termination::Crash, d, Some(side)));
            }
            SearchOutcome::Reply(r) => r,
        };

        let elapsed = reply.at.saturating_duration_since(go_time);
        let elapsed_us = elapsed.as_micros() as i64;
        let elapsed_ms = elapsed.as_millis() as u64;
        if is_clock {
            clock_us[si] -= elapsed_us;
            if clock_us[si] < -margin_us {
                g.rec.clock_end_ms = Some((clock_us[0] / 1000, clock_us[1] / 1000));
                let r = forfeit_result(&g.board, side, Termination::Time);
                let d = format!(
                    "{} lost on time: used {} ms, clock {} ms (bestmove {} not played)",
                    who,
                    elapsed_ms,
                    clock_us[si] / 1000,
                    reply.bestmove
                );
                return Some(g.finish(r, Termination::Time, d, Some(side)));
            }
            clock_us[si] += inc_us;
        }

        let mv = match san::parse_uci(&g.board, &reply.bestmove) {
            Some(m) => m,
            None => {
                if is_clock {
                    g.rec.clock_end_ms = Some((clock_us[0] / 1000, clock_us[1] / 1000));
                }
                let d = if reply.bestmove == "0000" || reply.bestmove == "(none)" || reply.bestmove.is_empty() {
                    format!("{} returned null move '{}' with legal moves available", who, reply.bestmove)
                } else if let Some(h) = san::nonstandard_castle_hint(&g.board, &reply.bestmove) {
                    format!("{} played illegal move '{}' ({})", who, reply.bestmove, h)
                } else {
                    format!("{} played illegal move '{}'", who, reply.bestmove)
                };
                return Some(g.finish(GameResult::win_for(!side), Termination::Illegal, d, Some(side)));
            }
        };

        g.push(mv, reply.score, reply.depth, elapsed_ms, reply.book);
        if let Some((r, t, d)) = g.check_end(settings.maxplies) {
            if is_clock {
                g.rec.clock_end_ms = Some((clock_us[0] / 1000, clock_us[1] / 1000));
            }
            return Some(g.finish(r, t, d, None));
        }

        // ---- optional resign adjudication: consecutive engine moves where the
        // mover's score agrees on the same winner by at least `cp`.
        if let Some(rs) = settings.resign {
            match reply.score {
                Some(s) => {
                    let white_view = if side == Color::White { s } else { -s };
                    if white_view.abs() >= rs.cp {
                        let sign = white_view.signum();
                        if sign == resign_sign {
                            resign_count += 1;
                        } else {
                            resign_sign = sign;
                            resign_count = 1;
                        }
                    } else {
                        resign_sign = 0;
                        resign_count = 0;
                    }
                }
                None => {
                    resign_sign = 0;
                    resign_count = 0;
                }
            }
            if resign_count >= 2 * rs.moves {
                if is_clock {
                    g.rec.clock_end_ms = Some((clock_us[0] / 1000, clock_us[1] / 1000));
                }
                let winner = if resign_sign > 0 { Color::White } else { Color::Black };
                let d = format!(
                    "resign adjudication: both engines scored >= {} cp for {} ({}) over {} consecutive moves each",
                    rs.cp,
                    color_name(winner),
                    names[idx(winner)],
                    rs.moves
                );
                return Some(g.finish(GameResult::win_for(winner), Termination::Resign, d, None));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tc_parse() {
        assert_eq!(Tc::parse("10+0.1").unwrap(), Tc::Clock { base_ms: 10000, inc_ms: 100 });
        assert_eq!(Tc::parse("10+0.1").unwrap().label(), "10+0.1");
        assert_eq!(Tc::parse("3+0.05").unwrap().label(), "3+0.05");
        assert_eq!(Tc::parse("60").unwrap().label(), "60+0");
        assert_eq!(Tc::parse("nodes=5000").unwrap(), Tc::Nodes(5000));
        assert_eq!(Tc::parse("movetime=100").unwrap().label(), "movetime=100");
        assert_eq!(Tc::parse("depth=8").unwrap(), Tc::Depth(8));
        assert!(Tc::parse("abc").is_err());
        assert!(Tc::parse("0+0").is_err());
    }

    fn spec(opening: &[&str]) -> GameSpec {
        GameSpec {
            id: 1,
            pairing: 0,
            game_pair: 0,
            a_is_white: true,
            white: "a".into(),
            black: "b".into(),
            opening_index: 3,
            opening: opening.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn rules_via_ongoing() {
        // fool's mate inside the "opening"
        let s = spec(&["f2f3", "e7e5", "g2g4", "d8h4"]);
        let mut g = Ongoing::new(&s, &Tc::Nodes(1));
        for u in &s.opening {
            let m = san::parse_uci(&g.board, u).unwrap();
            g.push(m, None, None, 0, false);
        }
        let (r, t, _) = g.check_end(600).unwrap();
        assert_eq!((r, t), (GameResult::BlackWins, Termination::Checkmate));
        assert_eq!(g.rec.san.last().unwrap(), "Qh4#");
        // threefold by knight shuffling
        let s = spec(&["g1f3", "g8f6", "f3g1", "f6g8", "g1f3", "g8f6", "f3g1", "f6g8"]);
        let mut g = Ongoing::new(&s, &Tc::Nodes(1));
        let mut ended = None;
        for u in &s.opening {
            let m = san::parse_uci(&g.board, u).unwrap();
            g.push(m, None, None, 0, false);
            if ended.is_none() {
                ended = g.check_end(600).map(|x| (x.1, g.rec.moves.len()));
            }
        }
        assert_eq!(ended, Some((Termination::Threefold, 8)));
        // time-forfeit draw rule: opponent with bare king / K+minor cannot win on time
        let b = Board::from_fen("8/8/8/4k3/8/8/2n5/R3K3 w - - 0 1", false).unwrap();
        assert_eq!(forfeit_result(&b, Color::White, Termination::Time), GameResult::Draw);
        assert_eq!(forfeit_result(&b, Color::Black, Termination::Time), GameResult::WhiteWins);
        assert_eq!(forfeit_result(&b, Color::White, Termination::Illegal), GameResult::BlackWins);
        // forfeit record
        let rec = forfeit_record(&spec(&["e2e4"]), &Tc::Nodes(5), Color::Black, "x".into());
        assert_eq!(rec.result, GameResult::WhiteWins);
        assert_eq!(rec.opening_plies, 1);
        let back = GameRecord::from_json(&Json::parse(&rec.to_json().to_compact()).unwrap()).unwrap();
        assert_eq!(back, rec);
    }
}
