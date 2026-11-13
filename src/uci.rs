use crate::book::Book;
use crate::search::{self, Limits};
use cozy_chess::{Board, Color};
use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const SEARCH_STACK: usize = 64 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Opt-in debug log: if a file named `aggro_debug.log` exists in the engine's
// working directory, every line received (>>) and sent (<<) is appended to it.
// ---------------------------------------------------------------------------
static LOG: std::sync::Mutex<Option<std::fs::File>> = std::sync::Mutex::new(None);
static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
/// Ponder state: 0 = not pondering, 1 = pondering (waiting for `ponderhit`/`stop`),
/// 2 = `ponderhit` seen - the ponder search stops and the real clocked search takes over.
pub static PONDER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// The clocked limits of the `go ponder` in flight, applied when `ponderhit` arrives.
static PONDER_LIMITS: std::sync::RwLock<Option<search::Limits>> = std::sync::RwLock::new(None);

/// Hand-relay continuity. A GUI that relays moves by hand may send every position as a bare
/// FEN with no move list (`position fen <board>`). Every per-game sacrifice statistic -- the
/// win-sac ritual, chaining, the after-sacrifice decay, the decided-game pressure boost and
/// threefold detection -- is computed from that move list, so a FEN-only relay would reset
/// it on every move and the engine would believe it had never sacrificed. This remembers the
/// position each search started from, the history behind it and the move it answered with,
/// so the next FEN can be recognised as the transport of it (our move plus the reply) and
/// the history continues instead of starting over.
#[derive(Clone)]
struct RelayAnchor {
    board: Board,
    mat_hist: Vec<i32>,
    movers: Vec<Color>,
    hashes: Vec<u64>,
    mv: Option<cozy_chess::Move>,
}
static RELAY: std::sync::Mutex<Option<RelayAnchor>> = std::sync::Mutex::new(None);

/// Record the move a search answered with, for hand-relay continuity (see `RelayAnchor`).
fn relay_note_move(mv: cozy_chess::Move) {
    if let Ok(mut g) = RELAY.lock() {
        if let Some(a) = g.as_mut() {
            a.mv = Some(mv);
        }
    }
}

/// If `new_board` is exactly the position after the anchor's own move and one opponent
/// reply, return the extended history: material before each move, who moved, and position
/// keys, with our move and the reply appended. The anchor's own position repeats the anchor
/// unchanged (a GUI re-asking for the same FEN), and the position after only our own move
/// also continues (the operator screenshotted before the reply). `None` for anything else.
fn relay_transport(
    a: &RelayAnchor,
    new_board: &Board,
) -> Option<(Vec<i32>, Vec<Color>, Vec<u64>, bool)> {
    // the very position the anchor holds: keep its history exactly as it was
    if search::rep_key(&a.board) == search::rep_key(new_board) {
        return Some((a.mat_hist.clone(), a.movers.clone(), a.hashes.clone(), false));
    }
    let mv = a.mv?;
    if !a.board.is_legal(mv) {
        return None;
    }
    let mut after = a.board.clone();
    after.play_unchecked(mv);
    let key_mid = search::rep_key(&after);
    let want = search::rep_key(new_board);
    let mut mat_hist = a.mat_hist.clone();
    let mut movers = a.movers.clone();
    let mut hashes = a.hashes.clone();
    // our own move
    movers.push(a.board.side_to_move());
    mat_hist.push(sac_material(&after));
    hashes.push(key_mid);
    if key_mid == want {
        return Some((mat_hist, movers, hashes, true));
    }
    let mut reply = None;
    after.generate_moves(|mvs| {
        for r in mvs {
            let mut b2 = after.clone();
            b2.play_unchecked(r);
            if search::rep_key(&b2) == want {
                reply = Some(r);
                return true;
            }
        }
        false
    });
    let reply = reply?;
    // the opponent's reply lands on the board we were given
    let mut final_board = after.clone();
    final_board.play_unchecked(reply);
    movers.push(after.side_to_move());
    mat_hist.push(sac_material(&final_board));
    hashes.push(want);
    Some((mat_hist, movers, hashes, true))
}

fn init_log() {
    START.get_or_init(std::time::Instant::now);
    if std::path::Path::new("aggro_debug.log").exists() {
        if let Ok(f) = std::fs::OpenOptions::new().append(true).open("aggro_debug.log") {
            if let Ok(mut l) = LOG.lock() {
                *l = Some(f);
            }
            log_line("==", "engine started");
        }
    }
}

fn log_line(dir: &str, s: &str) {
    if let Ok(mut l) = LOG.lock() {
        if let Some(f) = l.as_mut() {
            let ms = START.get().map(|t| t.elapsed().as_millis()).unwrap_or(0);
            let _ = writeln!(f, "{:>9} {} {}", ms, dir, s);
        }
    }
}

/// Write one line to the GUI (stdout) and to the debug log.
pub fn out(s: &str) {
    let stdout = io::stdout();
    let mut h = stdout.lock();
    let _ = writeln!(h, "{}", s);
    let _ = h.flush();
    drop(h);
    log_line("<<", s);
}

pub fn uci_loop() {
    let mut board = Board::default();
    let mut book_path = String::from("ph-gambitbook.bin");
    let mut book: Option<Book> = None; // loaded lazily so `uci` answers instantly
    let own_book = true; // curated gambits always; Polyglot book used if BookPath exists
    let mut move_overhead: u64 = 50; // ms reserved per move for GUI / network lag
    let mut history_hashes = vec![search::rep_key(&board)];
    let mut prev_plies: usize = 0;
    // the destination square of the opponent's last move, when it captured something
    // (a clean recapture on that square is a no-brainer for the human timing)
    let mut last_capture_to: Option<cozy_chess::Square> = None;

    let stop_flag = Arc::new(AtomicBool::new(false));
    let mut search_thread: Option<std::thread::JoinHandle<()>> = None;

    init_log();
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        log_line(">>", line.trim_end());
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }

        match tokens[0] {
            "uci" => {
                out!("id name AggroChess Ultimate Sacrificial Edition {}", env!("CARGO_PKG_VERSION"));
                out!("id author Rinarto Gibson (AggroChess); Ultimate Sacrificial Edition modifications");
                for line in option_lines() {
                    out!("{}", line);
                }
                out!("uciok");
            }
            "isready" => {
                // allocate the hash table now (after a Hash change), not on the clock of the
                // first move; a running search already has its table
                if search_thread.is_none() {
                    let _ = search::get_global_tt(search::TT_SIZE_MB.load(Ordering::Relaxed));
                    // the opening book too (it used to load on the first timed `go`, on the clock)
                    if own_book && book.is_none() {
                        book = Some(Book::new(&book_path));
                    }
                }
                out!("readyok");
            }
            "setoption" => {
                // setoption name <name...> value <value...>
                let name_pos = tokens.iter().position(|&t| t == "name");
                let value_pos = tokens.iter().position(|&t| t == "value");
                if let Some(np) = name_pos {
                    // a GUI may send `setoption value V name N` (value first): be robust,
                    // panic=abort would otherwise kill the whole engine on a bad ordering
                    let name_end = match value_pos {
                        Some(vp) if vp > np => vp,
                        _ => tokens.len(),
                    };
                    let (name, value) = if name_end > np + 1 {
                        (tokens[np + 1..name_end].join(" ").to_lowercase(), value_pos.map(|vp| tokens[vp + 1..].join(" ")).unwrap_or_default())
                    } else {
                        // name is where the value would start (or nothing): the line is
                        // malformed; the value (if any) still applies to the known option
                        (String::new(), String::new())
                    };
                    // The advertised options are user settings. Everything else (swindle mode,
                    // the tuning knobs) is fixed; the knobs answer only in a development run
                    // (AGGRO_DEV set), for testing.
                    let dev = std::env::var_os("AGGRO_DEV").is_some();
                    let name = if dev || is_user_option(&name) { name } else { String::new() };
                    let int = |v: &str| v.trim().parse::<i64>().ok();
                    match name.as_str() {
                        "threads" => {
                            if let Some(t) = int(&value) {
                                search::NUM_THREADS.store(t.clamp(1, 128) as usize, Ordering::Relaxed);
                            }
                        }
                        "hash" => {
                            if let Some(mb) = int(&value) {
                                search::TT_SIZE_MB.store(mb.clamp(1, 4096) as usize, Ordering::Relaxed);
                            }
                        }
                        "clear hash" => {
                            stop_and_join(&stop_flag, &mut search_thread);
                            search::clear_global_tt();
                        }
                        "multipv" => {
                            if let Some(v) = int(&value) {
                                search::MULTI_PV.store(v.clamp(1, 64) as usize, Ordering::Relaxed);
                            }
                        }
                        "move overhead" => {
                            if let Some(v) = int(&value) {
                                move_overhead = v.clamp(0, 5000) as u64;
                            }
                        }
                        "brilliance" => {
                            if let Some(v) = int(&value) {
                                search::BRILLIANCE.store(v.clamp(0, 100) as i32, Ordering::Relaxed);
                            }
                        }
                        "human think time" => {
                            if let Some(v) = int(&value) {
                                crate::human::THINK.store(v.clamp(0, 100) as i32, Ordering::Relaxed);
                            }
                        }
                        "human base minutes" => {
                            if let Some(v) = int(&value) {
                                crate::human::BASE_MINUTES.store(v.clamp(0, 480) as i32, Ordering::Relaxed);
                            }
                        }
                        "human increment seconds" => {
                            if let Some(v) = int(&value) {
                                crate::human::INCREMENT_SECONDS.store(v.clamp(0, 60) as i32, Ordering::Relaxed);
                            }
                        }
                        "human style" => {
                            if let Some(v) = int(&value) {
                                crate::human::STYLE.store(v.clamp(0, 100) as i32, Ordering::Relaxed);
                            }
                        }
                        "relay hint" => {
                            if let Some(v) = int(&value) {
                                crate::human::RELAY_HINT.store(v.clamp(0, 10_000) as i32, Ordering::Relaxed);
                            }
                        }
                        "skill level" | "skill_level" => {
                            if let Some(v) = int(&value) {
                                crate::skill::set_level(v.clamp(1, 21) as i32);
                            }
                        }
                        "aggression" | "sacrifice appetite" | "king attack priority" | "material preservation"
                        | "speculative sacrifices" => {
                            if let Some(v) = int(&value) {
                                let v = v.clamp(0, 100) as i32;
                                let mut p = crate::personality::current();
                                match name.as_str() {
                                    "aggression" => p.aggression = v,
                                    "sacrifice appetite" => p.sacrifice_appetite = v,
                                    "king attack priority" => p.king_attack = v,
                                    "material preservation" => p.material_preservation = v,
                                    _ => p.speculative = v,
                                }
                                if p != crate::personality::current() {
                                    crate::personality::set(p);
                                    // hash-table scores and cached evaluations hold the old personality
                                    search::clear_global_tt();
                                }
                            }
                        }
                        "x_skillseed" => {
                            if let Some(v) = int(&value) {
                                crate::skill::FIXED_SEED.store(v.max(0) as u64, Ordering::Relaxed);
                                crate::skill::reseed();
                            }
                        }
                        "bookpath" => {
                            book_path = value.clone();
                            book = None;
                        }
                        "swindle" => {
                            search::SWINDLE.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        // hidden tuning knobs (not advertised in "uci"; used for testing)
                        "x_nnuescale" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                crate::nnue::OUT_SCALE.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_sacbonus" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::SACRIFICE_BONUS.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_aftersac" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::AFTER_SAC_BONUS.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_deadbishop" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::DEAD_BISHOP.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_rootbelow" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::ROOT_FULL_BELOW.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_rootdraw" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::ROOT_DRAW_ESCAPE.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_stalebox" => {
                            search::STALEMATE_BOX.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        "x_rescue" => {
                            search::RESCUE.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        "x_rescuebelow" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::RESCUE_BELOW.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_rescuepct" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::RESCUE_PCT.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_rescuemax" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::RESCUE_MAX.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_stalescale" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                crate::eval::STALEMATE_BOX_SCALE.store(v.clamp(0, 64), Ordering::Relaxed);
                            }
                        }
                        "x_rescuequick" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::RESCUE_QUICK_PCT.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_rescuedrop" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::RESCUE_DROP.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_resmax" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::ROOT_RESEARCH_MAX.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_winsac" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::WIN_SAC_AT.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_lookpct" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::BRILLIANCE_PCT.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_lookfloor" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::LOOK_FLOOR_DELTA.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_lookmdiv" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::LOOK_MARGIN_DIV.store(v.max(1), Ordering::Relaxed);
                            }
                        }
                        "x_lookcap" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::LOOK_QUIET_CAP.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_looks2cap" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::LOOK_S2_CAP.store(v.max(0), Ordering::Relaxed);
                            }
                        }
                        "x_vetoslack" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::LOOK_VETO_SLACK.store(v.max(0), Ordering::Relaxed);
                            }
                        }
                        "x_lookveto" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::LOOK_VETO_ABS.store(v.max(0), Ordering::Relaxed);
                            }
                        }
                        "x_lookvetobad" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::LOOK_VETO_BAD.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_sacgroup" => {
                            let _ = value; // retired: the widened pair measured worse (43 vs 52 played)
                        }
                        "x_bookbril" => {
                            search::BOOK_BRILLIANT.store(value.trim() == "true" || value.trim() == "1", Ordering::Relaxed);
                        }
                        "x_winsacfloor" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::WIN_SAC_FLOOR.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_winsacnow" => {
                            search::WIN_SAC_NOW.store(value.trim() == "true" || value.trim() == "1", Ordering::Relaxed);
                        }
                        "x_nullverify" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::NULL_VERIFY.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_nullvdepth" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::NULL_VERIFY_DEPTH.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_zugz" => {
                            search::ZUGZWANG_GUARD.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        "x_sealed" => {
                            search::SEALED.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        "x_clamp" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                { crate::eval::KING_CLAMP.store(v, Ordering::Relaxed);
                                  crate::eval::PERS_VERSION.fetch_add(1, Ordering::Relaxed); }
                            }
                        }
                        "x_chain" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::CHAIN_BONUS.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_chainmin" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::CHAIN_MIN.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_chainmoves" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::CHAIN_MOVES.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_sacfloor" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::SAC_FLOOR.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_aggression" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                crate::eval::AGGRESSION.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_contempt" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::CONTEMPT.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_sacattack" => {
                            search::SAC_NEEDS_ATTACK.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        "x_talsacs" => {
                            search::TAL_SACS.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        "x_fortress" => {
                            search::FORTRESS.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        "x_talbelow" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::TAL_MODE_BELOW.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_egblend" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::EG_BLEND.store(v.clamp(0, 100), Ordering::Relaxed);
                            }
                        }
                        "x_eglmr" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::EG_LMR.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_drawesc" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::DRAW_ESCAPE.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_ocb" => {
                            search::OCB_SCALE.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        "x_eglook" => {
                            search::ENDGAME_LOOK.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        "x_lookquiet" => {
                            search::TAL_LOOK_QUIET.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        "x_lookmin" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::TAL_LOOK_MIN_MS.store(v.max(0), Ordering::Relaxed);
                            }
                        }
                        "x_deepms" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::DEEP_MIN_MS.store(v.max(0), Ordering::Relaxed);
                            }
                        }
                        "x_looktt" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::LOOK_TT_MB.store(v.clamp(1, 1024), Ordering::Relaxed);
                            }
                        }
                        "x_mateprobe" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::MATE_PROBE.store(v.max(0), Ordering::Relaxed);
                            }
                        }
                        "x_mpdepth" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::MATE_PROBE_DEPTH.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_looktal" => {
                            search::LOOK_TAL.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        "x_mpshare" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::MATE_PROBE_SHARE.store(v.max(0), Ordering::Relaxed);
                            }
                        }
                        "x_sacpvply" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::SAC_PV_PLY.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_prime" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::PRIME.store(v > 0, Ordering::Relaxed);
                                search::PRIME_PCT.store(v.max(1), Ordering::Relaxed);
                            }
                        }
                        "x_lookprime" => {
                            search::LOOK_PRIME.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        "x_lookpromising" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::TAL_LOOK_PCT_PROMISING.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_lookprimepct" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::LOOK_PRIME_PCT.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_mpdeficit" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::MATE_PROBE_DEFICIT.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_egwide" => {
                            search::EG_BLEND_WIDE.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        "x_netalways" => {
                            search::NET_ALWAYS.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        "x_zonelmr" => {
                            search::ZONE_SAC_LMR.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        "x_zoneorder" => {
                            search::ZONE_SAC_ORDER.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        "x_tallook" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::TAL_LOOK.store(v > 0, Ordering::Relaxed);
                                search::TAL_LOOK_PCT.store(v.max(1), Ordering::Relaxed);
                            }
                        }
                        "x_sacpv" => {
                            search::SAC_PV.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        "x_matenet" => {
                            search::MATE_NET.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        "x_sacfull" => {
                            search::SAC_FULL_DEPTH.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        "x_sacverify" => {
                            search::SAC_VERIFY.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        "x_sacunits" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::SAC_FULL_UNITS.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_pressure" => {
                            search::SAC_PRESSURE.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        "x_pstart" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::PRESSURE_START.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_pstep" => {
                            if let Ok(v) = value.trim().parse::<i32>() {
                                search::PRESSURE_STEP.store(v, Ordering::Relaxed);
                            }
                        }
                        "x_nnue" => {
                            search::USE_NNUE.store(value.trim().eq_ignore_ascii_case("true"), Ordering::Relaxed);
                        }
                        _ => {} // all other settings are fixed
                    }
                }
            }
            "ucinewgame" => {
                stop_and_join(&stop_flag, &mut search_thread);
                PONDER.store(0, Ordering::SeqCst);
                *PONDER_LIMITS.write().unwrap() = None;
                search::LAST_SCORE.store(0, Ordering::Relaxed);
                search::LAST_OBJ.store(0, Ordering::Relaxed);
                crate::skill::new_game();
                prev_plies = 0;
                board = Board::default();
                if let Ok(mut g) = RELAY.lock() {
                    *g = None;
                }
                history_hashes = vec![search::rep_key(&board)];
                search::clear_global_tt();
            }
            "position" => {
                stop_and_join(&stop_flag, &mut search_thread);
                history_hashes.clear();
                last_capture_to = None;
                let mut moves_index = None;
                if tokens.len() >= 2 && tokens[1] == "startpos" {
                    board = Board::default();
                    if tokens.len() >= 3 && tokens[2] == "moves" {
                        moves_index = Some(3);
                    }
                } else if tokens.len() >= 2 && tokens[1] == "fen" {
                    let mut i = 2;
                    let mut parts = Vec::new();
                    while i < tokens.len() && tokens[i] != "moves" {
                        parts.push(tokens[i]);
                        i += 1;
                    }
                    match Board::from_fen(&parts.join(" "), false) {
                        Ok(b) => board = b,
                        Err(e) => out!("info string Error parsing FEN: {:?}", e),
                    }
                    if i < tokens.len() {
                        moves_index = Some(i + 1);
                    }
                }
                history_hashes.push(search::rep_key(&board));
                // A `position fen <base> moves ...` keeps `base` around: hand relays often send
                // the board they started from plus the moves played on it, and that base may
                // be the continuation of the game we already know (see below).
                let fen_base: Option<Board> =
                    if tokens.len() >= 2 && tokens[1] == "fen" { Some(board.clone()) } else { None };
                // material (White's view) before each move, and who moved, for sacrifice tracking
                let mut mat_hist = vec![sac_material(&board)];
                let mut movers = Vec::new();
                if let Some(start) = moves_index {
                    for &mv_str in &tokens[start.min(tokens.len())..] {
                        match cozy_chess::util::parse_uci_move(&board, mv_str) {
                            Ok(mv) if board.is_legal(mv) => {
                                movers.push(board.side_to_move());
                                // the human timing wants to know if this move grabbed something
                                last_capture_to = if board.colors(!board.side_to_move()).has(mv.to) {
                                    Some(mv.to)
                                } else {
                                    None
                                };
                                board.play_unchecked(mv);
                                history_hashes.push(search::rep_key(&board));
                                mat_hist.push(sac_material(&board));
                            }
                            _ => {
                                out!("info string Illegal move in position: {}", mv_str);
                                break;
                            }
                        }
                    }
                }
                // Hand relay: the position a relay sends may still be the continuation of the
                // one we last searched, either as a bare FEN or as a base FEN plus the moves
                // played on it. Splice the remembered history back in so the game's sacrifice
                // state survives the relay instead of resetting on every move.
                let mut relay_note: Option<String> = None;
                let anchor = RELAY.lock().ok().and_then(|g| g.as_ref().cloned());
                if let Some(a) = anchor {
                    if search::rep_key(&a.board) == search::rep_key(&board) {
                        // the position we already hold, however it was spelled: keep the
                        // history we recorded for it (also what makes a re-query harmless)
                        mat_hist = a.mat_hist.clone();
                        movers = a.movers.clone();
                        history_hashes = a.hashes.clone();
                    } else if moves_index.is_none() {
                        // `history_hashes` currently holds only this FEN's key
                        if let Some((mh, mv2, hs, fresh)) = relay_transport(&a, &board) {
                            mat_hist = mh;
                            movers = mv2;
                            history_hashes = hs;
                            if fresh {
                                relay_note = Some(format!("info string relay: history continued ({} plies)", movers.len()));
                            }
                        }
                    } else if !movers.is_empty() {
                        // `position fen <base> moves <...>`: if the base continues our game,
                        // put our history in front of the moves this command supplied. Here
                        // the identical-board prefix counts too: the base is often exactly
                        // the position we last searched, with the moves played on it.
                        if let Some(fb) = fen_base.as_ref() {
                            if let Some((mut mh, mut mv2, mut hs, _)) = relay_transport(&a, fb) {
                                mh.extend_from_slice(&mat_hist[1..]);
                                mv2.extend_from_slice(&movers);
                                hs.extend_from_slice(&history_hashes[1..]);
                                mat_hist = mh;
                                movers = mv2;
                                history_hashes = hs;
                                relay_note = Some(format!("info string relay: history continued ({} plies)", movers.len()));
                            }
                        }
                    }
                }
                let (sacs, last_sac) = count_sacrifices_ex(&mat_hist, &movers);
                search::GAME_SACS[0].store(sacs[0], Ordering::Relaxed);
                search::GAME_SACS[1].store(sacs[1], Ordering::Relaxed);
                for side in 0..2 {
                    // own moves since that side's latest sacrifice (1 = its previous move)
                    let ago = if last_sac[side] < 0 { 1000 } else { ((movers.len() as i64 - last_sac[side]) + 1) / 2 };
                    search::LAST_SAC_AGO[side].store(ago as i32, Ordering::Relaxed);
                }
                // a shorter game than last time means a new game: forget the old score
                if movers.len() < prev_plies || movers.len() < 2 {
                    search::LAST_SCORE.store(0, Ordering::Relaxed);
                    search::LAST_OBJ.store(0, Ordering::Relaxed);
                }
                // ...and, for GUIs that never send ucinewgame, a fresh Skill Level allowance (only
                // for games given as moves: a FEN-only GUI sends every position "fresh")
                if moves_index.is_some() && movers.len() < prev_plies {
                    crate::skill::new_game();
                }
                prev_plies = movers.len();
                if let Some(note) = relay_note {
                    out!("{}", note);
                }
                if std::env::var_os("AGGRO_RELAYDBG").is_some() {
                    out!(
                        "info string relay state: plies {} sacs [{}, {}] ago [{}, {}]",
                        movers.len(),
                        search::GAME_SACS[0].load(Ordering::Relaxed),
                        search::GAME_SACS[1].load(Ordering::Relaxed),
                        search::LAST_SAC_AGO[0].load(Ordering::Relaxed),
                        search::LAST_SAC_AGO[1].load(Ordering::Relaxed)
                    );
                }
                // remember this position so a following bare FEN can continue the history.
                // A re-send of the very position already held keeps its recorded move, so a
                // re-query cannot break the relay chain.
                if let Ok(mut g) = RELAY.lock() {
                    let kept = g.as_ref().and_then(|a| {
                        if search::rep_key(&a.board) == search::rep_key(&board) { a.mv } else { None }
                    });
                    *g = Some(RelayAnchor {
                        board: board.clone(),
                        mat_hist,
                        movers,
                        hashes: history_hashes.clone(),
                        mv: kept,
                    });
                }
            }
            "go" => {
                stop_and_join(&stop_flag, &mut search_thread);

                let mut limits = Limits::default();
                let mut wtime = None;
                let mut btime = None;
                let mut winc = 0u64;
                let mut binc = 0u64;
                let mut movestogo: Option<u64> = None;
                let mut movetime = None;
                let mut infinite = false;
                let mut ponder = false;
                let mut searchmoves: Vec<cozy_chess::Move> = Vec::new();
                let mut i = 1;
                while i < tokens.len() {
                    let next = tokens.get(i + 1).copied().unwrap_or("");
                    match tokens[i] {
                        "infinite" => infinite = true,
                        "ponder" => {
                            // the GUI pondered for us: search open-ended until `ponderhit`
                            // (which hands the real clock over) or `stop`
                            ponder = true;
                        }
                        "depth" => {
                            limits.depth = next.parse().ok();
                            i += 1;
                        }
                        "nodes" => {
                            limits.nodes = next.parse().ok();
                            i += 1;
                        }
                        "movetime" => {
                            movetime = next.parse::<u64>().ok();
                            i += 1;
                        }
                        "wtime" => {
                            wtime = next.parse::<i64>().ok().map(|v| v.max(0) as u64);
                            i += 1;
                        }
                        "btime" => {
                            btime = next.parse::<i64>().ok().map(|v| v.max(0) as u64);
                            i += 1;
                        }
                        "winc" => {
                            winc = next.parse().unwrap_or(0);
                            i += 1;
                        }
                        "binc" => {
                            binc = next.parse().unwrap_or(0);
                            i += 1;
                        }
                        "movestogo" => {
                            movestogo = next.parse().ok();
                            i += 1;
                        }
                        "searchmoves" => {
                            let mut f = Vec::new();
                            while let Some(t) = tokens.get(i + 1) {
                                match cozy_chess::util::parse_uci_move(&board, t) {
                                    Ok(mv) if board.is_legal(mv) => {
                                        f.push(mv);
                                        i += 1;
                                    }
                                    _ => break,
                                }
                            }
                            searchmoves = f;
                        }
                        _ => {}
                    }
                    i += 1;
                }
                *search::ROOT_FILTER.lock().unwrap() = searchmoves.clone();

                // Opening book (only for normal timed games / plain "go"). A weakened engine
                // (Skill Level) leaves its book after a level-dependent number of plies.
                // `book_hold` is the book's move when the position is searched anyway because a
                // sacrifice candidate exists (see below): it is kept unless the look promotes.
                let mut book_hold: Option<cozy_chess::Move> = None;
                let game_ply = 2 * (board.fullmove_number().max(1) as u32 - 1) + (board.side_to_move() == Color::Black) as u32;
                let book_ok = crate::skill::params().map_or(true, |w| w.book_plies == 0 || game_ply < w.book_plies);
                if own_book && book_ok && limits.depth.is_none() && limits.nodes.is_none() && !infinite && !ponder && searchmoves.is_empty() {
                    if book.is_none() {
                        book = Some(Book::new(&book_path));
                    }
                    if let Some(bm) = book.as_ref().and_then(|b| b.get_move(&board)) {
                        if let Ok(mv) = cozy_chess::util::parse_uci_move(&board, &bm) {
                            if board.is_legal(mv) {
                                // Brilliance: a book position may hold a sound sacrifice, but
                                // the book branch returns before any search runs, so the
                                // brilliancy look never sees the first moves of the game. When
                                // Brilliance is on and the position has a static sacrifice
                                // candidate, search anyway; the book move is kept unless the
                                // look deliberately promotes a sacrifice over it.
                                let bril = search::BRILLIANCE.load(Ordering::Relaxed) > 0
                                    && search::BOOK_BRILLIANT.load(Ordering::Relaxed);
                                let sac_here = bril && {
                                    let mut root = Vec::new();
                                    board.generate_moves(|mvs| {
                                        root.extend(mvs);
                                        false
                                    });
                                    !crate::brilliant::candidates(&board, &root).is_empty()
                                };
                                if !sac_here {
                                    relay_note_move(mv);
                                    out!("info string book move");
                                    out!("bestmove {}", cozy_chess::util::display_uci_move(&board, mv));
                                    let _ = io::stdout().flush();
                                    continue;
                                }
                                // search it: fall through with the book move remembered
                                book_hold = Some(mv);
                            }
                        }
                    }
                }

                if let Some(mt) = movetime {
                    let t = Duration::from_millis(mt.saturating_sub(move_overhead).max(1));
                    limits.soft = Some(t);
                    limits.hard = Some(t);
                } else if !infinite {
                    let (time, inc) = if board.side_to_move() == Color::White { (wtime, winc) } else { (btime, binc) };
                    if let Some(time) = time {
                        let avail = time.saturating_sub(move_overhead).max(1);
                        let mtg = movestogo.unwrap_or(30).clamp(1, 50);
                        let mut soft = avail / mtg + inc * 3 / 4;
                        soft = soft.min(avail / 4);
                        // the last move before the time control gets a fresh clock right
                        // after it: plan most of this one, keep only the usual emergency
                        // reserve (a twelfth). `movestogo 1` must not think with a quarter
                        // of the clock while the whole game rides on this move.
                        if mtg == 1 {
                            soft = (avail + inc * 3 / 4) * 11 / 12;
                        }
                        let hard = (soft * 4).min(avail / 2).max(soft);
                        let hard = if mtg == 1 { (avail + inc * 3 / 4).min(hard * 2) } else { hard };
                        limits.soft = Some(Duration::from_millis(soft.max(1)));
                        limits.hard = Some(Duration::from_millis(hard.max(1)));
                    }
                }

                // Ponder: the GUI guessed the opponent's reply and wants us to think ahead
                // about *our* next move. Those clock numbers belong to the position on the
                // board, which only becomes current at `ponderhit` - until then the search
                // is open-ended and emits nothing. At `ponderhit` the real limits take
                // over on a hash table that is already warm with the ponder work.
                if ponder {
                    PONDER.store(1, Ordering::SeqCst);
                    *PONDER_LIMITS.write().unwrap() = Some(limits);
                    limits.soft = None;
                    limits.hard = None;
                    infinite = true; // no bestmove until the GUI decides (ponderhit/stop)
                } else {
                    PONDER.store(0, Ordering::SeqCst);
                }

                let search_board = board.clone();
                let stop_clone = Arc::clone(&stop_flag);
                let hist = history_hashes.clone();
                stop_flag.store(false, Ordering::SeqCst);
                search::EXTERNAL_STOP.store(false, Ordering::SeqCst);
                let wait_for_stop = infinite;
                // the Human Think Time act: what the timing layer needs to know about this go.
                // A fixed `go movetime` is a game too: the engine never sees the clock, so the
                // user's time-frame setting (Human Base Minutes, or 60 s as a fallback) stands
                // in for it and the pace acts normally on top of the fixed search time.
                let movetime_frame = movetime.map(|mt| {
                    // A fixed movetime game has no visible clock, so the pace needs a frame.
                    // The frame the operator declared (Human Base Minutes) is the game they
                    // are actually relaying into, so that is the clock the pauses pace
                    // against - the engine's small search budget is not the game's speed.
                    // Without a declared frame the movetime itself implies one (40 moves).
                    let mins = crate::human::BASE_MINUTES.load(Ordering::Relaxed).max(0);
                    if mins > 0 { 60_000 * mins as u64 } else { mt.saturating_mul(40) }
                });
                let human = crate::human::Limits {
                    ourtime: if infinite || !searchmoves.is_empty() {
                        // single-move analysis replies print at once (module doc)
                        None
                    } else if let Some(frame) = movetime_frame {
                        Some(frame)
                    } else {
                        let (t, i) = if board.side_to_move() == Color::White { (wtime, winc) } else { (btime, binc) };
                        if limits.depth.is_some() || limits.nodes.is_some() {
                            None
                        } else if t.is_none() {
                            // no clock reported at all: act in the frame the user set, if any
                            let mins = crate::human::BASE_MINUTES.load(Ordering::Relaxed).max(0);
                            if mins == 0 { None } else { Some(60_000 * mins as u64) }
                        } else {
                            t.map(|tt| tt.saturating_add(i))
                        }
                    },
                    inc: if movetime.is_some() {
                        crate::human::INCREMENT_SECONDS.load(Ordering::Relaxed).max(0) as u64 * 1000
                    } else if board.side_to_move() == Color::White { winc } else { binc },
                    movestogo,
                    book: false,
                    movetime,
                    last_capture_to,
                };
                // the bestmove reply is postponed by the Human Think Time act (0 = at once)
                let pondering = ponder;
                let fallback_board = board.clone();
                let spawn_result = std::thread::Builder::new()
                    .stack_size(SEARCH_STACK)
                    .spawn(move || {
                            // Ponder: think ahead open-ended in a thread of its own while the
                            // GUI makes up its mind. `ponderhit` converts that work into the
                            // real clocked search on the hash table the ponder just warmed;
                            // `stop` wants the move now (from whatever the ponder reached).
                            if pondering {
                                let ponder_stop = Arc::new(AtomicBool::new(false));
                                let pboard = search_board.clone();
                                let phist = hist.clone();
                                let pstop = Arc::clone(&ponder_stop);
                                let ponderer = std::thread::Builder::new()
                                    .stack_size(SEARCH_STACK)
                                    .spawn(move || {
                                        // open-ended: think as long as the GUI lets us
                                        let lim = search::Limits { depth: None, soft: None, hard: None, nodes: None };
                                        let (b, _) = search::search_position(&pboard, &phist, lim, pstop, true);
                                        b
                                    })
                                    .expect("failed to spawn ponder thread");
                                loop {
                                    if PONDER.load(Ordering::SeqCst) != 1 {
                                        break;
                                    }
                                    std::thread::sleep(Duration::from_millis(2));
                                }
                                // either ponderhit (2) or stop (0): end the ponder search
                                ponder_stop.store(true, Ordering::SeqCst);
                                search::EXTERNAL_STOP.store(true, Ordering::SeqCst);
                                let ponder_best = ponderer.join().ok().flatten();
                                if PONDER.load(Ordering::SeqCst) == 2 {
                                    // ponderhit: the position is real now; search it again
                                    // under the stored clock (the shared TT is warm)
                                    search::EXTERNAL_STOP.store(false, Ordering::SeqCst);
                                    stop_clone.store(false, Ordering::SeqCst);
                                    let limits_run = PONDER_LIMITS.read().unwrap().clone().unwrap_or(limits);
                                    let (best, _) = search::search_position(&search_board, &hist, limits_run, Arc::clone(&stop_clone), true);
                                    match best {
                                        Some(mv) => out!("bestmove {}", cozy_chess::util::display_uci_move(&search_board, mv)),
                                        None => out!("bestmove 0000"),
                                    }
                                    let _ = io::stdout().flush();
                                    return;
                                }
                                // stopped during ponder: answer with what the ponder has
                                search::EXTERNAL_STOP.store(false, Ordering::SeqCst);
                                stop_clone.store(false, Ordering::SeqCst);
                                match ponder_best {
                                    Some(mv) => out!("bestmove {}", cozy_chess::util::display_uci_move(&search_board, mv)),
                                    None => out!("bestmove 0000"),
                                }
                                let _ = io::stdout().flush();
                                return;
                            }
                            let t0 = std::time::Instant::now();
                            let (best, _) = search::search_position(&search_board, &hist, limits, Arc::clone(&stop_clone), true);
                            let think_ms = t0.elapsed().as_millis();
                            // `go infinite`: the GUI decides when the search is over, even when
                            // the search itself has finished (a mate found, a spent budget)
                            while wait_for_stop && !search::EXTERNAL_STOP.load(Ordering::SeqCst) {
                                std::thread::sleep(Duration::from_millis(2));
                            }
                            if !wait_for_stop {
                                let comp = search::LAST_COMPLEXITY.swap(0, Ordering::Relaxed);
                                let wait = crate::human::delay_ms(&search_board, best, &human, think_ms, comp);
                                if wait > 0 {
                                    if std::env::var_os("AGGRO_HUMANDBG").is_some() {
                                        out!("info string human think: waiting {} ms (complexity {}, search {} ms)", wait, comp, think_ms);
                                    }
                                    // Relay Hint: for hand-relay setups, the move is shown to
                                    // the operator this many ms before the think ends (`0` =
                                    // never shown early). Useful only when a human relays the
                                    // engine's moves by hand; the bestmove itself is unchanged.
                                    let hint = crate::human::RELAY_HINT.load(Ordering::Relaxed).clamp(0, 10_000) as u64;
                                    let mut hint_done = hint == 0;
                                    // interruptible: a `stop` that arrives during the act ends it
                                    // at once (stop_and_join sets EXTERNAL_STOP first)
                                    let until = std::time::Instant::now() + Duration::from_millis(wait);
                                    while std::time::Instant::now() < until && !search::EXTERNAL_STOP.load(Ordering::SeqCst) {
                                        std::thread::sleep(Duration::from_millis(2));
                                        if !hint_done && until.duration_since(std::time::Instant::now()).as_millis() <= u128::from(hint) {
                                            if let Some(mv) = best {
                                                out!("info string relay hint: {}", cozy_chess::util::display_uci_move(&search_board, mv));
                                            }
                                            hint_done = true;
                                        }
                                    }
                                }
                            }
                            match best {
                                Some(mv) => {
                                    // Brilliance/book: the position was searched only because a
                                    // sacrifice candidate existed, so unless the look promoted a
                                    // sacrifice the book move still stands.
                                    let mv = match book_hold {
                                        Some(bm) if !search::LOOK_PLAYED.load(Ordering::Relaxed) => bm,
                                        _ => mv,
                                    };
                                    relay_note_move(mv);
                                    out!("bestmove {}", cozy_chess::util::display_uci_move(&search_board, mv))
                                }
                                None => out!("bestmove 0000"),
                            }
                            let _ = io::stdout().flush();
                        });
                    match spawn_result {
                        Ok(handle) => search_thread = Some(handle),
                        Err(e) => {
                            // panic=abort would kill the engine for the whole rest of the
                            // session: a failed spawn (thread/memory exhaustion) must not.
                            // Answer at once from the first legal move so the game continues.
                            let mut first: Option<cozy_chess::Move> = None;
                            fallback_board.generate_moves(|mvs| {
                                if first.is_none() {
                                    first = mvs.into_iter().next();
                                }
                                false
                            });
                            out!("info string search thread unavailable: {} - playing a legal move", e);
                            match first {
                                Some(mv) => out!("bestmove {}", cozy_chess::util::display_uci_move(&fallback_board, mv)),
                                None => out!("bestmove 0000"),
                            }
                            let _ = io::stdout().flush();
                        }
                    }
                }
            "ponderhit" => {
                // the pondered position became reality: hand the stored clock over and
                // release the waiting search. The ponder search runs open-ended; this is
                // the transition to the timed one.
                if PONDER.load(Ordering::SeqCst) == 1 {
                    PONDER.store(2, Ordering::SeqCst);
                    search::EXTERNAL_STOP.store(true, Ordering::SeqCst);
                    stop_flag.store(true, Ordering::SeqCst);
                }
            }
            "stop" => {
                PONDER.store(0, Ordering::SeqCst);
                stop_and_join(&stop_flag, &mut search_thread);
            }
            "quit" => {
                PONDER.store(0, Ordering::SeqCst);
                stop_and_join(&stop_flag, &mut search_thread);
                break;
            }
            "d" => {
                out!("{}", board);
                out!("eval (side to move): {} (nnue {}, personality {}, classical {})", crate::nnue::evaluate_fresh(&board) + crate::eval::personality(&board), crate::nnue::evaluate_fresh(&board), crate::eval::personality(&board), crate::eval::evaluate(&board));
            }
            _ => {}
        }
        let _ = io::stdout().flush();
    }
}

/// Material from White's point of view (pawn 100, knight 320, bishop 330, rook 500, queen 900).
fn sac_material(board: &Board) -> i32 {
    const VAL: [i32; 6] = [100, 320, 330, 500, 900, 0];
    let mut s = 0;
    for p in cozy_chess::Piece::ALL {
        s += VAL[p as usize]
            * (board.colored_pieces(cozy_chess::Color::White, p).len() as i32
                - board.colored_pieces(cozy_chess::Color::Black, p).len() as i32);
    }
    s
}

/// Sacrifices made by each colour in a game: the mover is at least 2 pawns down two plies
/// later and still at least 1.5 pawns down two plies after that (the same rule as the match
/// statistics). `mat_hist[k]` is the material before move `k`, `movers[k]` who made it.
pub fn count_sacrifices(mat_hist: &[i32], movers: &[cozy_chess::Color]) -> [i32; 2] {
    count_sacrifices_ex(mat_hist, movers).0
}

/// Sacrifices per side, and for each side the ply index of its latest sacrifice (-1: none).
pub fn count_sacrifices_ex(mat_hist: &[i32], movers: &[cozy_chess::Color]) -> ([i32; 2], [i64; 2]) {
    let mut sacs = [0; 2];
    let mut last = [-1i64; 2];
    // each side on its own, so one side's sacrifice never hides the other's
    for side in [cozy_chess::Color::White, cozy_chess::Color::Black] {
        let sign = if side == cozy_chess::Color::White { 1 } else { -1 };
        let mut k = 0;
        while k < movers.len() {
            if movers[k] != side {
                k += 1;
                continue;
            }
            let before = mat_hist[k] * sign;
            if k + 2 < mat_hist.len() {
                let after2 = mat_hist[k + 2] * sign;
                let after4 = if k + 4 < mat_hist.len() { mat_hist[k + 4] * sign } else { after2 };
                if after2 <= before - 200 && after4 <= before - 150 {
                    sacs[side as usize] += 1;
                    last[side as usize] = k as i64;
                    // a follow-up sacrifice within the next two moves is part of the same
                    // sequence: it extends the chain (latest sacrifice) without a new count
                    let mut j = k + 2;
                    while j < k + 4 && j < movers.len() {
                        if movers[j] == side && j + 2 < mat_hist.len() && mat_hist[j + 2] * sign <= mat_hist[j] * sign - 200 {
                            last[side as usize] = j as i64;
                        }
                        j += 2;
                    }
                    k += 4;
                    continue;
                }
            }
            k += 1;
        }
    }
    (sacs, last)
}

fn stop_and_join(stop: &Arc<AtomicBool>, handle: &mut Option<std::thread::JoinHandle<()>>) {
    if let Some(h) = handle.take() {
        // the GUI's stop first (seen by every search and by `go infinite`), then the flag
        search::EXTERNAL_STOP.store(true, Ordering::SeqCst);
        stop.store(true, Ordering::SeqCst);
        let _ = h.join();
    }
    stop.store(false, Ordering::SeqCst);
}

/// The options advertised to the GUI.
pub fn option_lines() -> Vec<String> {
    let p = crate::personality::DEFAULTS;
    vec![
        format!("option name Threads type spin default {} min 1 max 128", search::NUM_THREADS.load(Ordering::Relaxed)),
        "option name Hash type spin default 128 min 1 max 4096".to_string(),
        "option name Clear Hash type button".to_string(),
        "option name MultiPV type spin default 1 min 1 max 64".to_string(),
        "option name Move Overhead type spin default 50 min 0 max 5000".to_string(),
        format!("option name Skill Level type spin default {} min {} max {}", crate::skill::FULL, crate::skill::MIN, crate::skill::FULL),
        format!("option name Aggression type spin default {} min 0 max 100", p.aggression),
        format!("option name Sacrifice Appetite type spin default {} min 0 max 100", p.sacrifice_appetite),
        format!("option name King Attack Priority type spin default {} min 0 max 100", p.king_attack),
        format!("option name Material Preservation type spin default {} min 0 max 100", p.material_preservation),
        format!("option name Speculative Sacrifices type spin default {} min 0 max 100", p.speculative),
        format!("option name Brilliance type spin default {} min 0 max 100", search::BRILLIANCE_DEFAULT),
        "option name Human Think Time type spin default 0 min 0 max 100".to_string(),
        "option name Human Base Minutes type spin default 0 min 0 max 480".to_string(),
        "option name Human Increment Seconds type spin default 0 min 0 max 60".to_string(),
        "option name Human Style type spin default 0 min 0 max 100".to_string(),
        "option name Relay Hint type spin default 0 min 0 max 10000".to_string(),
        "option name BookPath type string default ph-gambitbook.bin".to_string(),
    ]
}

/// Whether `name` (lowercase) is an advertised option (or an accepted alias).
fn is_user_option(name: &str) -> bool {
    matches!(
        name,
        "threads"
            | "hash"
            | "clear hash"
            | "multipv"
            | "move overhead"
            | "skill level"
            | "skill_level"
            | "aggression"
            | "sacrifice appetite"
            | "king attack priority"
            | "material preservation"
            | "speculative sacrifices"
            | "brilliance"
            | "human think time"
            | "human base minutes"
            | "human increment seconds"
            | "human style"
            | "relay hint"
            | "bookpath"
    )
}

#[cfg(test)]
mod relay_tests {
    use super::*;
    use cozy_chess::Board;

    fn mv(b: &Board, uci: &str) -> cozy_chess::Move {
        cozy_chess::util::parse_uci_move(b, uci).expect("legal test move")
    }

    fn anchor_after(moves: &[&str]) -> RelayAnchor {
        let mut board = Board::default();
        let mut mat_hist = vec![sac_material(&board)];
        let mut movers = Vec::new();
        let mut hashes = vec![search::rep_key(&board)];
        for m in moves {
            let m = mv(&board, m);
            movers.push(board.side_to_move());
            board.play_unchecked(m);
            mat_hist.push(sac_material(&board));
            hashes.push(search::rep_key(&board));
        }
        RelayAnchor { board, mat_hist, movers, hashes, mv: None }
    }

    #[test]
    fn relay_continues_after_our_move_and_a_reply() {
        let mut a = anchor_after(&["e2e4", "e7e5", "g1f3", "b8c6"]);
        a.mv = Some(mv(&a.board, "f1b5"));
        let mut after = a.board.clone();
        after.play_unchecked(a.mv.unwrap());
        after.play_unchecked(mv(&after, "a7a6"));
        let (mat, movers, hashes, fresh) =
            relay_transport(&a, &after).expect("our move plus the reply must splice");
        assert!(fresh);
        assert_eq!(movers.len(), 6, "4 plies + our move + the reply");
        assert_eq!(hashes.len(), mat.len());
        assert_eq!(*hashes.last().unwrap(), search::rep_key(&after));
        assert_eq!(movers[4], cozy_chess::Color::White);
        assert_eq!(movers[5], cozy_chess::Color::Black);
    }

    #[test]
    fn relay_accepts_the_board_of_our_own_move_alone() {
        let mut a = anchor_after(&["e2e4"]);
        a.mv = Some(mv(&a.board, "d7d5"));
        let mut after = a.board.clone();
        after.play_unchecked(a.mv.unwrap());
        let (_, movers, _, fresh) = relay_transport(&a, &after).expect("our move alone splices");
        assert!(fresh);
        assert_eq!(movers.len(), 2);
    }

    #[test]
    fn relay_repeats_the_anchor_position_unchanged() {
        let mut a = anchor_after(&["e2e4", "e7e5"]);
        a.mv = Some(mv(&a.board, "g1f3"));
        let (mat, movers, hashes, fresh) =
            relay_transport(&a, &a.board.clone()).expect("the anchor's own board matches");
        assert!(!fresh, "a re-send must not be announced as new history");
        assert_eq!(movers, a.movers);
        assert_eq!(mat, a.mat_hist);
        assert_eq!(hashes, a.hashes);
    }

    #[test]
    fn relay_refuses_a_position_two_replies_away() {
        let mut a = anchor_after(&["e2e4", "e7e5"]);
        a.mv = Some(mv(&a.board, "g1f3"));
        // g1f3, b8c6, f1b5: our move and TWO replies
        let mut far = a.board.clone();
        for m in ["g1f3", "b8c6", "f1b5"] {
            let m = mv(&far, m);
            far.play_unchecked(m);
        }
        assert!(relay_transport(&a, &far).is_none(),
                "only one reply may be bridged, so a longer gap must not splice");
    }

    #[test]
    fn relay_keeps_the_sacrifice_count_across_a_spliced_history() {
        // a real sequence with a piece sacrifice: 5...Nxe4-style give-away
        let a = anchor_after(&["d2d4", "d7d5", "c2c4", "e7e6", "b1c3", "g8f6", "c4d5", "e6d5"]);
        let mut a = a;
        a.mv = Some(mv(&a.board, "c3d5")); // Nxd5: heavy material for a pawn
        let mut after = a.board.clone();
        after.play_unchecked(a.mv.unwrap());
        let reply = mv(&after, "f6d5");
        after.play_unchecked(reply);
        let (mat, movers, _, _) = relay_transport(&a, &after).expect("splices");
        let (sacs, _) = count_sacrifices_ex(&mat, &movers);
        assert!(sacs[0] > 0, "the spliced history must remember the sacrifice");
    }
}
