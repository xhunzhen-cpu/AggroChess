//! `match` subcommand: config parsing, scheduling, worker threads, outputs.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use cozy_chess::Color;

use crate::engine::{Engine, EngineConfig};
use crate::game::{self, GameRecord, GameResult, GameSettings, GameSpec, Resign, Tc, Termination};
use crate::json::Json;
use crate::openings;
use crate::pgn;
use crate::stats;
use crate::sys;

pub const UCIOK_TIMEOUT: Duration = Duration::from_secs(10);
pub const READY_TIMEOUT: Duration = Duration::from_secs(30);
/// Default for `poolsize`: idle engine processes a worker may keep *in addition to*
/// the ones its next game needs (LRU). 0 = an engine the next game does not use is
/// shut down, so a worker never holds more than the 2 engines of its current game.
pub const DEFAULT_POOL_EXTRA: usize = 0;

#[derive(Debug, Clone)]
pub struct PairLine {
    pub a: String,
    pub b: String,
    pub games: usize,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub path: PathBuf,
    pub engines: Vec<EngineConfig>,
    pub tc: Tc,
    pub concurrency: usize,
    pub openings: Option<PathBuf>,
    pub seed: u64,
    pub maxplies: usize,
    pub timemargin_ms: u64,
    pub resign: Option<Resign>,
    pub pgn: Option<PathBuf>,
    pub jsonl: Option<PathBuf>,
    pub pairs: Vec<PairLine>,
    pub event: String,
    /// Idle engines a worker may keep beyond those its next game needs.
    pub pool_extra: usize,
}

impl Config {
    pub fn engine(&self, name: &str) -> Option<&EngineConfig> {
        self.engines.iter().find(|e| e.name == name)
    }
}

fn strip_comment(line: &str) -> &str {
    // '#' starts a comment at line start or after whitespace (so paths/values
    // containing '#' elsewhere survive)
    let b = line.as_bytes();
    for i in 0..b.len() {
        if b[i] == b'#' && (i == 0 || b[i - 1].is_ascii_whitespace()) {
            return &line[..i];
        }
    }
    line
}

/// `rest` after removing the first `n` whitespace-separated words.
fn rest_after_words(line: &str, n: usize) -> &str {
    let mut s = line.trim_start();
    for _ in 0..n {
        match s.find(char::is_whitespace) {
            Some(p) => s = s[p..].trim_start(),
            None => return "",
        }
    }
    s.trim_end()
}

fn resolve(base: &Path, p: &str) -> PathBuf {
    let pb = PathBuf::from(p);
    if pb.is_absolute() {
        pb
    } else {
        base.join(pb)
    }
}

pub fn parse_config(path: &Path) -> Result<Config, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    // relative paths in the config are resolved against the config file's directory
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let base_buf = abs.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
    let base = base_buf.as_path();
    let mut engines: Vec<EngineConfig> = Vec::new();
    let mut options: Vec<(usize, String, String, String)> = Vec::new();
    let mut cwds: Vec<(usize, String, String)> = Vec::new();
    let mut tc: Option<Tc> = None;
    let mut cfg = Config {
        path: abs.clone(),
        engines: Vec::new(),
        tc: Tc::Nodes(1),
        concurrency: 1,
        openings: None,
        seed: 1,
        maxplies: 600,
        timemargin_ms: 100,
        resign: None,
        pgn: None,
        jsonl: None,
        pairs: Vec::new(),
        event: "aggro_arena match".to_string(),
        pool_extra: DEFAULT_POOL_EXTRA,
    };
    for (ln0, raw) in text.lines().enumerate() {
        let ln = ln0 + 1;
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        let err = |m: &str| format!("{}:{}: {}", path.display(), ln, m);
        let need = |n: usize| -> Result<(), String> {
            if words.len() < n {
                Err(err(&format!("'{}' needs {} argument(s)", words[0], n - 1)))
            } else {
                Ok(())
            }
        };
        match words[0] {
            "engine" => {
                need(3)?;
                if engines.iter().any(|e| e.name == words[1]) {
                    return Err(err(&format!("engine '{}' declared twice", words[1])));
                }
                let p = words[2];
                let exe = if p.contains('/') { resolve(base, p).to_string_lossy().into_owned() } else { p.to_string() };
                let mut e = EngineConfig::new(words[1], &exe);
                e.args = words[3..].iter().map(|s| s.to_string()).collect();
                engines.push(e);
            }
            "option" => {
                need(3)?;
                let rest = rest_after_words(line, 2);
                let (k, v) = rest.split_once('=').ok_or_else(|| err("option needs '<UCI name> = <value>'"))?;
                if k.trim().is_empty() {
                    return Err(err("empty option name"));
                }
                options.push((ln, words[1].to_string(), k.trim().to_string(), v.trim().to_string()));
            }
            "cwd" => {
                need(3)?;
                cwds.push((ln, words[1].to_string(), rest_after_words(line, 2).to_string()));
            }
            "tc" => {
                need(2)?;
                tc = Some(Tc::parse(rest_after_words(line, 1)).map_err(|e| err(&e))?);
            }
            "concurrency" => {
                need(2)?;
                cfg.concurrency = words[1].parse().map_err(|_| err("bad concurrency"))?;
                if cfg.concurrency == 0 || cfg.concurrency > 256 {
                    return Err(err("concurrency must be 1..256"));
                }
            }
            "openings" => {
                need(2)?;
                cfg.openings = Some(resolve(base, rest_after_words(line, 1)));
            }
            "seed" => {
                need(2)?;
                cfg.seed = words[1].parse().map_err(|_| err("bad seed"))?;
            }
            "maxplies" => {
                need(2)?;
                cfg.maxplies = words[1].parse().map_err(|_| err("bad maxplies"))?;
            }
            "timemargin" => {
                need(2)?;
                cfg.timemargin_ms = words[1].parse().map_err(|_| err("bad timemargin"))?;
            }
            "resign" => {
                need(3)?;
                let cp: i32 = words[1].parse().map_err(|_| err("bad resign cp"))?;
                let moves: u32 = words[2].parse().map_err(|_| err("bad resign moves"))?;
                if cp <= 0 || moves == 0 {
                    return Err(err("resign needs cp > 0 and moves > 0"));
                }
                cfg.resign = Some(Resign { cp, moves });
            }
            "pgn" => {
                need(2)?;
                cfg.pgn = Some(resolve(base, rest_after_words(line, 1)));
            }
            "jsonl" => {
                need(2)?;
                cfg.jsonl = Some(resolve(base, rest_after_words(line, 1)));
            }
            "event" => {
                need(2)?;
                cfg.event = rest_after_words(line, 1).to_string();
            }
            "poolsize" => {
                need(2)?;
                cfg.pool_extra = words[1].parse().map_err(|_| err("bad poolsize"))?;
                if cfg.pool_extra > 64 {
                    return Err(err("poolsize must be 0..64"));
                }
            }
            "pair" => {
                need(4)?;
                let games: usize = words[3].parse().map_err(|_| err("bad game count"))?;
                if games == 0 {
                    return Err(err("pair needs at least 1 game"));
                }
                cfg.pairs.push(PairLine { a: words[1].to_string(), b: words[2].to_string(), games: games + games % 2 });
            }
            other => return Err(err(&format!("unknown directive '{}'", other))),
        }
    }
    for (ln, name, k, v) in options {
        let e = engines
            .iter_mut()
            .find(|e| e.name == name)
            .ok_or_else(|| format!("{}:{}: option for undeclared engine '{}'", path.display(), ln, name))?;
        if let Some(slot) = e.options.iter_mut().find(|(kk, _)| kk.eq_ignore_ascii_case(&k)) {
            slot.1 = v;
        } else {
            e.options.push((k, v));
        }
    }
    for (ln, name, dir) in cwds {
        let e = engines
            .iter_mut()
            .find(|e| e.name == name)
            .ok_or_else(|| format!("{}:{}: cwd for undeclared engine '{}'", path.display(), ln, name))?;
        e.cwd = Some(resolve(base, &dir));
    }
    cfg.tc = tc.ok_or_else(|| format!("{}: missing 'tc'", path.display()))?;
    if cfg.pairs.is_empty() {
        return Err(format!("{}: no 'pair' lines", path.display()));
    }
    for p in &cfg.pairs {
        for n in [&p.a, &p.b] {
            if !engines.iter().any(|e| &e.name == n) {
                return Err(format!("{}: pair uses undeclared engine '{}'", path.display(), n));
            }
        }
    }
    cfg.engines = engines;
    Ok(cfg)
}

// ---------------------------------------------------------------- scheduling

struct SplitMix(u64);
impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        // rejection sampling for an unbiased index
        let zone = u64::MAX - (u64::MAX % n);
        loop {
            let v = self.next();
            if v < zone {
                return v % n;
            }
        }
    }
}

/// Build the game queue: each pairing gets ceil(games/2) distinct openings
/// (seeded shuffle, taken sequentially across pairings); every opening is
/// played twice with colours reversed; pairings are interleaved pair by pair.
pub fn build_schedule(cfg: &Config, openings: &[openings::Opening]) -> (Vec<GameSpec>, bool) {
    let mut order: Vec<usize> = (0..openings.len()).collect();
    let mut rng = SplitMix(cfg.seed);
    for i in (1..order.len()).rev() {
        let j = rng.below(i as u64 + 1) as usize;
        order.swap(i, j);
    }
    let mut next_open = 0usize;
    let mut wrapped = false;
    let mut per_pairing: Vec<Vec<(i64, Vec<String>)>> = Vec::new();
    for p in &cfg.pairs {
        let mut v = Vec::new();
        for _ in 0..p.games / 2 {
            if openings.is_empty() {
                v.push((-1, Vec::new()));
            } else {
                if next_open >= order.len() {
                    wrapped = true;
                }
                let o = &openings[order[next_open % order.len()]];
                next_open += 1;
                v.push((o.index, o.moves.clone()));
            }
        }
        per_pairing.push(v);
    }
    let mut specs = Vec::new();
    let max_pairs = per_pairing.iter().map(|v| v.len()).max().unwrap_or(0);
    for k in 0..max_pairs {
        for (pi, p) in cfg.pairs.iter().enumerate() {
            if let Some((oi, moves)) = per_pairing[pi].get(k) {
                for a_white in [true, false] {
                    let (w, b) = if a_white { (&p.a, &p.b) } else { (&p.b, &p.a) };
                    specs.push(GameSpec {
                        id: specs.len() as u64 + 1,
                        pairing: pi,
                        game_pair: k,
                        a_is_white: a_white,
                        white: w.clone(),
                        black: b.clone(),
                        opening_index: *oi,
                        opening: moves.clone(),
                    });
                }
            }
        }
    }
    (specs, wrapped)
}

// ---------------------------------------------------------------- workers

/// Idle engine processes of one worker. Before each game `prepare` shuts down the
/// idle engines that game does not use (keeping at most `extra` of them, most
/// recently used first), so a worker holds at most 2 + `extra` processes: with many
/// engine names in one config (calibration batches) the arena no longer accumulates
/// idle Stockfish processes (~150 MB each) that are never used again.
struct Pool {
    idle: Vec<(Engine, u64)>,
    tick: u64,
    extra: usize,
}

impl Pool {
    fn take(&mut self, name: &str) -> Option<Engine> {
        let pos = self.idle.iter().position(|(e, _)| e.name == name)?;
        Some(self.idle.swap_remove(pos).0)
    }
    fn put(&mut self, mut e: Engine) {
        if e.dead || !e.is_alive() {
            return; // dropped => killed
        }
        self.tick += 1;
        self.idle.push((e, self.tick));
        // never more than the 2 engines of a game plus `extra`
        self.evict_lru_while(|p| p.idle.len() > 2 + p.extra);
    }
    /// Keep the idle engines `needed` will take (up to one per occurrence of a name)
    /// plus at most `extra` others; shut the rest down.
    fn prepare(&mut self, needed: &[&str]) {
        let mut want: Vec<&str> = needed.to_vec();
        let mut keep: Vec<(Engine, u64)> = Vec::new();
        let mut others: Vec<(Engine, u64)> = Vec::new();
        for (e, t) in self.idle.drain(..) {
            if let Some(i) = want.iter().position(|n| *n == e.name) {
                want.swap_remove(i);
                keep.push((e, t));
            } else {
                others.push((e, t));
            }
        }
        others.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
        while others.len() > self.extra {
            let (old, _) = others.pop().unwrap();
            drop(old); // quit / kill
        }
        keep.extend(others);
        self.idle = keep;
    }
    fn evict_lru_while(&mut self, cond: impl Fn(&Pool) -> bool) {
        while cond(self) && !self.idle.is_empty() {
            let (i, _) = self.idle.iter().enumerate().min_by_key(|(_, (_, t))| *t).unwrap();
            let (old, _) = self.idle.swap_remove(i);
            drop(old);
        }
    }
}

fn acquire(pool: &mut Pool, ecfg: &EngineConfig, worker: usize) -> Result<Engine, String> {
    let mut last_err = String::from("unknown error");
    for attempt in 0..3 {
        if sys::stop_requested() {
            return Err("stop requested".into());
        }
        if attempt > 0 {
            thread::sleep(Duration::from_millis(250));
        }
        let mut e = match pool.take(&ecfg.name) {
            Some(e) => e,
            None => match Engine::start(ecfg, UCIOK_TIMEOUT, READY_TIMEOUT) {
                Ok(e) => e,
                Err(err) => {
                    errln!("[worker {}] engine '{}' failed to start: {}", worker, ecfg.name, err);
                    last_err = err;
                    continue;
                }
            },
        };
        match e.send("ucinewgame").and_then(|_| e.isready(READY_TIMEOUT)) {
            Ok(()) => return Ok(e),
            Err(err) => {
                errln!("[worker {}] engine '{}' not ready ({}), restarting", worker, ecfg.name, err);
                last_err = err;
                e.kill();
            }
        }
    }
    Err(last_err)
}

struct Shared {
    cfg: Config,
    specs: Vec<GameSpec>,
    next: AtomicUsize,
    settings: GameSettings,
}

fn worker(wid: usize, sh: Arc<Shared>, tx: mpsc::Sender<GameRecord>) {
    let mut pool = Pool { idle: Vec::new(), tick: 0, extra: sh.cfg.pool_extra };
    loop {
        if sys::stop_requested() {
            break;
        }
        let i = sh.next.fetch_add(1, Ordering::SeqCst);
        if i >= sh.specs.len() {
            break;
        }
        let spec = &sh.specs[i];
        let wcfg = sh.cfg.engine(&spec.white).expect("validated");
        let bcfg = sh.cfg.engine(&spec.black).expect("validated");
        pool.prepare(&[spec.white.as_str(), spec.black.as_str()]);
        let w = acquire(&mut pool, wcfg, wid);
        let b = acquire(&mut pool, bcfg, wid);
        if sys::stop_requested() {
            break;
        }
        let rec = match (w, b) {
            (Ok(mut w), Ok(mut b)) => {
                let r = game::play_game(spec, &mut w, &mut b, &sh.settings);
                pool.put(w);
                pool.put(b);
                r
            }
            (Err(e), other) => {
                if let Ok(o) = other {
                    pool.put(o);
                }
                Some(game::forfeit_record(spec, &sh.settings.tc, Color::White, format!("white ({}) unavailable: {}", spec.white, e)))
            }
            (Ok(w), Err(e)) => {
                pool.put(w);
                Some(game::forfeit_record(spec, &sh.settings.tc, Color::Black, format!("black ({}) unavailable: {}", spec.black, e)))
            }
        };
        match rec {
            Some(r) => {
                if tx.send(r).is_err() {
                    break;
                }
            }
            None => break, // interrupted
        }
    }
    drop(pool); // quits/kills all engines of this worker
}

// ---------------------------------------------------------------- statistics

#[derive(Default, Clone)]
struct EngineTally {
    time: u64,
    illegal: u64,
    crash: u64,
    moves: u64,
    total_ms: u64,
    max_ms: u64,
}

struct PairStats {
    a: String,
    b: String,
    planned: usize,
    w: u64,
    d: u64,
    l: u64,
    white_wins: u64,
    black_wins: u64,
    plies: u64,
    terms: BTreeMap<&'static str, u64>,
    tally: [EngineTally; 2],
}

impl PairStats {
    fn new(p: &PairLine) -> PairStats {
        PairStats {
            a: p.a.clone(),
            b: p.b.clone(),
            planned: p.games,
            w: 0,
            d: 0,
            l: 0,
            white_wins: 0,
            black_wins: 0,
            plies: 0,
            terms: BTreeMap::new(),
            tally: [EngineTally::default(), EngineTally::default()],
        }
    }

    fn games(&self) -> u64 {
        self.w + self.d + self.l
    }

    fn add(&mut self, rec: &GameRecord, a_is_white: bool) {
        let a_color = if a_is_white { Color::White } else { Color::Black };
        let s = rec.result.score_for(a_color);
        if s == 1.0 {
            self.w += 1;
        } else if s == 0.0 {
            self.l += 1;
        } else {
            self.d += 1;
        }
        match rec.result {
            GameResult::WhiteWins => self.white_wins += 1,
            GameResult::BlackWins => self.black_wins += 1,
            GameResult::Draw => {}
        }
        self.plies += rec.moves.len() as u64;
        *self.terms.entry(rec.termination.as_str()).or_default() += 1;
        if let Some(side) = rec.forfeit_side {
            let who = if side == a_color { 0 } else { 1 };
            match rec.termination {
                Termination::Time => self.tally[who].time += 1,
                Termination::Illegal => self.tally[who].illegal += 1,
                Termination::Crash => self.tally[who].crash += 1,
                _ => {}
            }
        }
        for i in rec.opening_plies..rec.moves.len() {
            // ply parity: even plies are white's moves (games start from startpos)
            let mover_white = i % 2 == 0;
            let who = if mover_white == a_is_white { 0 } else { 1 };
            let t = rec.times_ms[i];
            self.tally[who].moves += 1;
            self.tally[who].total_ms += t;
            self.tally[who].max_ms = self.tally[who].max_ms.max(t);
        }
    }

    fn short(&self) -> String {
        let st = stats::wdl_stats(self.w, self.d, self.l);
        format!(
            "{} vs {}: +{} ={} -{} ({:.1}%, Elo {})",
            self.a,
            self.b,
            self.w,
            self.d,
            self.l,
            st.score * 100.0,
            stats::fmt_elo(st.elo)
        )
    }

    fn report(&self) -> String {
        let st = stats::wdl_stats(self.w, self.d, self.l);
        let mut s = String::new();
        s.push_str(&format!("=== {} vs {} ({} of {} games) ===\n", self.a, self.b, self.games(), self.planned));
        s.push_str(&format!(
            "  {}: W {} / D {} / L {}   score {:.1}%   draws {:.1}%\n",
            self.a,
            self.w,
            self.d,
            self.l,
            st.score * 100.0,
            st.draw_ratio * 100.0
        ));
        let pm = match (st.elo_lo, st.elo_hi) {
            (Some(lo), Some(hi)) => format!("  95% CI [{:+.1}, {:+.1}] (±{:.1})", lo, hi, (hi - lo) / 2.0),
            (lo, hi) => format!("  95% CI [{}, {}]", stats::fmt_elo(lo).replace("n/a", "-inf"), stats::fmt_elo(hi).replace("n/a", "+inf")),
        };
        s.push_str(&format!(
            "  Elo diff ({} - {}): {}{}   LOS {}\n",
            self.a,
            self.b,
            stats::fmt_elo(st.elo),
            pm,
            st.los.map(|x| format!("{:.1}%", x * 100.0)).unwrap_or_else(|| "n/a".into())
        ));
        s.push_str(&format!(
            "  white wins {}, black wins {}, avg plies {:.1}\n",
            self.white_wins,
            self.black_wins,
            if self.games() > 0 { self.plies as f64 / self.games() as f64 } else { 0.0 }
        ));
        let terms: Vec<String> = self.terms.iter().map(|(k, v)| format!("{} {}", k, v)).collect();
        s.push_str(&format!("  terminations: {}\n", terms.join(", ")));
        for (i, name) in [&self.a, &self.b].iter().enumerate() {
            let t = &self.tally[i];
            s.push_str(&format!(
                "  {}: time forfeits {}, illegal {}, crash {}; avg move {:.0} ms, max {} ms\n",
                name,
                t.time,
                t.illegal,
                t.crash,
                if t.moves > 0 { t.total_ms as f64 / t.moves as f64 } else { 0.0 },
                t.max_ms
            ));
        }
        s
    }

    fn to_json(&self) -> Json {
        let st = stats::wdl_stats(self.w, self.d, self.l);
        let mut terms = Json::obj();
        for (k, v) in &self.terms {
            terms.set(k, *v);
        }
        let mut forfeits = Json::obj();
        let mut timing = Json::obj();
        for (i, name) in [&self.a, &self.b].iter().enumerate() {
            let t = &self.tally[i];
            let key = if self.a == self.b { format!("{}#{}", name, i + 1) } else { name.to_string() };
            forfeits.set(&key, Json::obj().with("time", t.time).with("illegal", t.illegal).with("crash", t.crash));
            timing.set(
                &key,
                Json::obj()
                    .with("moves", t.moves)
                    .with("avg_ms", if t.moves > 0 { t.total_ms as f64 / t.moves as f64 } else { 0.0 })
                    .with("max_ms", t.max_ms),
            );
        }
        Json::obj()
            .with("engine_a", &self.a)
            .with("engine_b", &self.b)
            .with("games_planned", self.planned)
            .with("games", self.games())
            .with("wins", self.w)
            .with("draws", self.d)
            .with("losses", self.l)
            .with("score", st.score)
            .with("draw_ratio", st.draw_ratio)
            .with("elo", st.elo)
            .with("elo_lo", st.elo_lo)
            .with("elo_hi", st.elo_hi)
            .with("score_se", st.score_se)
            .with("los", st.los)
            .with("white_wins", self.white_wins)
            .with("black_wins", self.black_wins)
            .with("avg_plies", if self.games() > 0 { self.plies as f64 / self.games() as f64 } else { 0.0 })
            .with("terminations", terms)
            .with("forfeits", forfeits)
            .with("timing", timing)
    }
}

fn summary_path(jsonl: &Path) -> PathBuf {
    let mut s = jsonl.as_os_str().to_os_string();
    s.push(".summary.json");
    PathBuf::from(s)
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

fn open_out(p: &Option<PathBuf>) -> Result<Option<BufWriter<File>>, String> {
    match p {
        None => Ok(None),
        Some(p) => {
            if let Some(dir) = p.parent() {
                if !dir.as_os_str().is_empty() {
                    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
                }
            }
            File::create(p).map(|f| Some(BufWriter::new(f))).map_err(|e| format!("{}: {}", p.display(), e))
        }
    }
}

// ---------------------------------------------------------------- command

pub fn cmd(args: &[String]) -> i32 {
    if args.len() != 1 || args[0] == "-h" || args[0] == "--help" {
        errln!("usage: aggro_arena match <config>   (see testing/README.md for the config format)");
        return if args.len() == 1 { 0 } else { 2 };
    }
    let cfg = match parse_config(Path::new(&args[0])) {
        Ok(c) => c,
        Err(e) => {
            errln!("config error: {}", e);
            return 2;
        }
    };
    match run(cfg) {
        Ok(code) => code,
        Err(e) => {
            errln!("error: {}", e);
            1
        }
    }
}

fn run(cfg: Config) -> Result<i32, String> {
    sys::install_signal_handlers();
    let t0 = Instant::now();
    let started = sys::now_iso_utc();

    let opening_list = match &cfg.openings {
        Some(p) => openings::load_openings(p)?,
        None => Vec::new(),
    };
    let (specs, wrapped) = build_schedule(&cfg, &opening_list);
    if wrapped {
        errln!("warning: more game pairs than openings; openings are reused");
    }
    let total = specs.len();
    let concurrency = cfg.concurrency.min(total.max(1));

    // ---- verify every engine once (fail fast on bad paths / handshakes)
    let mut used: Vec<&str> = Vec::new();
    for p in &cfg.pairs {
        for n in [p.a.as_str(), p.b.as_str()] {
            if !used.contains(&n) {
                used.push(n);
            }
        }
    }
    let mut engine_ids = Json::obj();
    for n in &used {
        let ec = cfg.engine(n).unwrap();
        let mut e = Engine::start(ec, UCIOK_TIMEOUT, READY_TIMEOUT).map_err(|e| format!("engine '{}': {}", n, e))?;
        for (k, _) in &ec.options {
            if !e.has_option(k) {
                errln!("warning: engine '{}' does not advertise option '{}'", n, k);
            }
        }
        let id = e.id_name.clone().unwrap_or_else(|| "?".into());
        outln!("engine {}: {} ({})", n, id, ec.path);
        engine_ids.set(n, Json::obj().with("id_name", id).with("path", &ec.path).with(
            "options",
            Json::Obj(ec.options.iter().map(|(k, v)| (k.clone(), Json::Str(v.clone()))).collect()),
        ));
        e.quit(Duration::from_millis(500));
    }

    outln!(
        "match: {} games, {} pairings, concurrency {}, tc {}, openings {}, seed {}",
        total,
        cfg.pairs.len(),
        concurrency,
        cfg.tc.label(),
        cfg.openings.as_ref().map(|p| format!("{} ({} valid)", p.display(), opening_list.len())).unwrap_or_else(|| "startpos".into()),
        cfg.seed
    );

    let mut pgn_out = open_out(&cfg.pgn)?;
    let mut jsonl_out = open_out(&cfg.jsonl)?;
    let site = sys::hostname();

    let settings = GameSettings { tc: cfg.tc, maxplies: cfg.maxplies, timemargin_ms: cfg.timemargin_ms, resign: cfg.resign };
    let shared = Arc::new(Shared { cfg: cfg.clone(), specs, next: AtomicUsize::new(0), settings });
    let (tx, rx) = mpsc::channel::<GameRecord>();
    let mut handles = Vec::new();
    for wid in 0..concurrency {
        let sh = Arc::clone(&shared);
        let txc = tx.clone();
        let h = thread::Builder::new()
            .name(format!("worker-{}", wid))
            .spawn(move || worker(wid, sh, txc))
            .map_err(|e| format!("cannot spawn worker: {}", e))?;
        handles.push(h);
    }
    drop(tx);

    let mut pstats: Vec<PairStats> = cfg.pairs.iter().map(PairStats::new).collect();
    let mut done = 0usize;
    let mut write_err: Option<String> = None;

    let make_summary = |pstats: &Vec<PairStats>, done: usize, finished: bool| -> Json {
        Json::obj()
            .with("config", cfg.path.to_string_lossy().into_owned())
            .with("started", started.clone())
            .with("updated", sys::now_iso_utc())
            .with("duration_s", (t0.elapsed().as_secs_f64() * 10.0).round() / 10.0)
            .with("tc", cfg.tc.label())
            .with("concurrency", concurrency)
            .with("poolsize", cfg.pool_extra)
            .with("seed", cfg.seed)
            .with("openings", cfg.openings.as_ref().map(|p| p.to_string_lossy().into_owned()))
            .with("games_total", total)
            .with("games_played", done)
            .with("complete", finished && done == total)
            .with("interrupted", sys::stop_requested())
            .with("engines", engine_ids.clone())
            .with("pairs", Json::Arr(pstats.iter().map(|p| p.to_json()).collect()))
    };

    for rec in rx.iter() {
        done += 1;
        let spec = &shared.specs[(rec.id - 1) as usize];
        let ps = &mut pstats[rec.pairing];
        ps.add(&rec, spec.a_is_white);
        outln!(
            "game {}/{} (#{}): {} vs {} {} {} | {}",
            done,
            total,
            rec.id,
            rec.white,
            rec.black,
            rec.result.as_str(),
            rec.termination.as_str(),
            ps.short()
        );
        if rec.termination.is_forfeit() {
            outln!("    -> {}", rec.detail);
        }
        if let Some(f) = pgn_out.as_mut() {
            let meta = pgn::PgnMeta { event: &cfg.event, site: &site, tc: &cfg.tc };
            if let Err(e) = f.write_all(pgn::write_game(&rec, &meta).as_bytes()).and_then(|_| f.flush()) {
                write_err = Some(format!("pgn write: {}", e));
            }
        }
        if let Some(f) = jsonl_out.as_mut() {
            let mut line = rec.to_json().to_compact();
            line.push('\n');
            if let Err(e) = f.write_all(line.as_bytes()).and_then(|_| f.flush()) {
                write_err = Some(format!("jsonl write: {}", e));
            }
        }
        if let Some(j) = &cfg.jsonl {
            let _ = write_atomic(&summary_path(j), &make_summary(&pstats, done, false).to_pretty());
        }
        if let Some(e) = &write_err {
            errln!("error: {}; stopping", e);
            sys::request_stop();
        }
        if sys::stdout_broken() && !sys::stop_requested() {
            // whoever read our progress output is gone (e.g. `| tee` killed by Ctrl-C, or the
            // driving script died): stop like on SIGINT so the summary is still finalised
            errln!("stdout closed; stopping (running games are abandoned)");
            sys::request_stop();
        }
    }
    for h in handles {
        if h.join().is_err() {
            errln!("error: a worker thread panicked");
        }
    }

    outln!();
    outln!("finished {} of {} games in {:.1} s", done, total, t0.elapsed().as_secs_f64());
    for p in &pstats {
        out!("{}", p.report());
    }
    if let Some(j) = &cfg.jsonl {
        let sp = summary_path(j);
        write_atomic(&sp, &make_summary(&pstats, done, true).to_pretty()).map_err(|e| format!("{}: {}", sp.display(), e))?;
        outln!("summary: {}", sp.display());
    }
    if write_err.is_some() {
        return Ok(1);
    }
    if sys::stop_requested() {
        errln!("interrupted");
        return Ok(130);
    }
    Ok(0)
}

// ---------------------------------------------------------------- summary of existing jsonl

/// `summary <games.jsonl>...`: recompute W/D/L + Elo per engine pairing from
/// existing JSONL files (games of A-vs-B and B-vs-A are merged).
pub fn summary_cmd(args: &[String]) -> i32 {
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        errln!("usage: aggro_arena summary <games.jsonl>...");
        return 2;
    }
    let mut groups: Vec<PairStats> = Vec::new();
    let mut n = 0usize;
    for a in args {
        let recs = match game::read_jsonl(Path::new(a)) {
            Ok(r) => r,
            Err(e) => {
                errln!("{}", e);
                return 1;
            }
        };
        for rec in recs {
            n += 1;
            let gi = match groups.iter().position(|g| (g.a == rec.white && g.b == rec.black) || (g.a == rec.black && g.b == rec.white)) {
                Some(i) => i,
                None => {
                    groups.push(PairStats::new(&PairLine { a: rec.white.clone(), b: rec.black.clone(), games: 0 }));
                    groups.len() - 1
                }
            };
            let g = &mut groups[gi];
            let a_is_white = g.a == rec.white;
            g.add(&rec, a_is_white);
        }
    }
    outln!("{} games", n);
    for g in &mut groups {
        g.planned = g.games() as usize;
        out!("{}", g.report());
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_and_schedule() {
        let dir = std::env::temp_dir().join(format!("arena_cfg_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let op = dir.join("ops.txt");
        std::fs::write(&op, "e2e4 e7e5\nd2d4 d7d5\nc2c4\ng1f3 g8f6\n").unwrap();
        let cfgp = dir.join("m.cfg");
        std::fs::write(
            &cfgp,
            "# comment\nengine sf /usr/games/stockfish\nengine x ./bin/x --flag\noption sf Skill Level = 3 # trailing\n\
             option sf Hash = 16\ncwd x sub dir\ntc 3+0.05\nconcurrency 4\nopenings ops.txt\nseed 42\n\
             pair sf x 3\npair x sf 4\njsonl out/g.jsonl\nresign 900 3\n",
        )
        .unwrap();
        let cfg = parse_config(&cfgp).unwrap();
        assert_eq!(cfg.engines.len(), 2);
        assert_eq!(cfg.engines[0].options, vec![("Skill Level".to_string(), "3".to_string()), ("Hash".to_string(), "16".to_string())]);
        assert_eq!(cfg.engines[1].args, vec!["--flag"]);
        assert_eq!(cfg.engines[1].path, dir.join("./bin/x").to_string_lossy());
        assert_eq!(cfg.engines[1].cwd.as_ref().unwrap(), &dir.join("sub dir"));
        assert_eq!(cfg.pairs[0].games, 4);
        assert_eq!(cfg.jsonl.as_ref().unwrap(), &dir.join("out/g.jsonl"));
        let ops = openings::load_openings(cfg.openings.as_ref().unwrap()).unwrap();
        let (specs, wrapped) = build_schedule(&cfg, &ops);
        assert_eq!(specs.len(), 8);
        assert!(!wrapped); // 4 openings, 2+2 game pairs
        // consecutive colour-reversed pairs sharing one opening; all openings distinct
        let mut seen = std::collections::HashSet::new();
        for ch in specs.chunks(2) {
            assert_eq!(ch[0].opening_index, ch[1].opening_index);
            assert_eq!((ch[0].white.as_str(), ch[0].black.as_str()), (ch[1].black.as_str(), ch[1].white.as_str()));
            assert!(ch[0].a_is_white && !ch[1].a_is_white);
            assert_eq!((ch[0].pairing, ch[0].game_pair), (ch[1].pairing, ch[1].game_pair));
            assert!(seen.insert(ch[0].opening_index));
        }
        // interleaved pairings and deterministic for a given seed
        assert_eq!(specs.iter().map(|s| s.pairing).collect::<Vec<_>>(), vec![0, 0, 1, 1, 0, 0, 1, 1]);
        let (specs2, _) = build_schedule(&cfg, &ops);
        assert_eq!(
            specs.iter().map(|s| s.opening_index).collect::<Vec<_>>(),
            specs2.iter().map(|s| s.opening_index).collect::<Vec<_>>()
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
