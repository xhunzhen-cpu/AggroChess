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
    let mut history_hashes = vec![board.hash()];
    let mut prev_plies: usize = 0;

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
                    let name_end = value_pos.unwrap_or(tokens.len());
                    let name = tokens[np + 1..name_end].join(" ").to_lowercase();
                    let value = value_pos.map(|vp| tokens[vp + 1..].join(" ")).unwrap_or_default();
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
                                crate::eval::KING_CLAMP.store(v, Ordering::Relaxed);
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
                search::LAST_SCORE.store(0, Ordering::Relaxed);
                search::LAST_OBJ.store(0, Ordering::Relaxed);
                crate::skill::new_game();
                prev_plies = 0;
                board = Board::default();
                history_hashes = vec![board.hash()];
                search::clear_global_tt();
            }
            "position" => {
                stop_and_join(&stop_flag, &mut search_thread);
                history_hashes.clear();
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
                history_hashes.push(board.hash());
                // material (White's view) before each move, and who moved, for sacrifice tracking
                let mut mat_hist = vec![sac_material(&board)];
                let mut movers = Vec::new();
                if let Some(start) = moves_index {
                    for &mv_str in &tokens[start.min(tokens.len())..] {
                        match cozy_chess::util::parse_uci_move(&board, mv_str) {
                            Ok(mv) if board.is_legal(mv) => {
                                movers.push(board.side_to_move());
                                board.play_unchecked(mv);
                                history_hashes.push(board.hash());
                                mat_hist.push(sac_material(&board));
                            }
                            _ => {
                                out!("info string Illegal move in position: {}", mv_str);
                                break;
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
                let mut searchmoves: Vec<cozy_chess::Move> = Vec::new();
                let mut i = 1;
                while i < tokens.len() {
                    let next = tokens.get(i + 1).copied().unwrap_or("");
                    match tokens[i] {
                        "infinite" => infinite = true,
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
                let game_ply = 2 * (board.fullmove_number().max(1) as u32 - 1) + (board.side_to_move() == Color::Black) as u32;
                let book_ok = crate::skill::params().map_or(true, |w| w.book_plies == 0 || game_ply < w.book_plies);
                if own_book && book_ok && limits.depth.is_none() && limits.nodes.is_none() && !infinite && searchmoves.is_empty() {
                    if book.is_none() {
                        book = Some(Book::new(&book_path));
                    }
                    if let Some(bm) = book.as_ref().and_then(|b| b.get_move(&board)) {
                        if let Ok(mv) = cozy_chess::util::parse_uci_move(&board, &bm) {
                            if board.is_legal(mv) {
                                out!("info string book move");
                                out!("bestmove {}", cozy_chess::util::display_uci_move(&board, mv));
                                let _ = io::stdout().flush();
                                continue;
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
                        let hard = (soft * 4).min(avail / 2).max(soft);
                        limits.soft = Some(Duration::from_millis(soft.max(1)));
                        limits.hard = Some(Duration::from_millis(hard.max(1)));
                    }
                }

                let search_board = board.clone();
                let stop_clone = Arc::clone(&stop_flag);
                let hist = history_hashes.clone();
                stop_flag.store(false, Ordering::SeqCst);
                search::EXTERNAL_STOP.store(false, Ordering::SeqCst);
                let wait_for_stop = infinite;
                // the Human Think Time act: what the timing layer needs to know about this go
                let human = crate::human::Limits {
                    ourtime: if infinite || movetime.is_some() {
                        None
                    } else {
                        let (t, i) = if board.side_to_move() == Color::White { (wtime, winc) } else { (btime, binc) };
                        if limits.depth.is_some() || limits.nodes.is_some() { None } else { t.map(|tt| tt.saturating_add(i)) }
                    },
                    inc: if board.side_to_move() == Color::White { winc } else { binc },
                    movestogo,
                    book: false,
                };
                // the bestmove reply is postponed by the Human Think Time act (0 = at once)
                search_thread = Some(
                    std::thread::Builder::new()
                        .stack_size(SEARCH_STACK)
                        .spawn(move || {
                            let t0 = std::time::Instant::now();
                            let (best, _) = search::search_position(&search_board, &hist, limits, stop_clone, true);
                            let think_ms = t0.elapsed().as_millis();
                            // `go infinite`: the GUI decides when the search is over, even when the
                            // search itself has finished (a mate found, a Skill Level budget spent)
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
                                    // interruptible: a `stop` that arrives during the act ends it
                                    // at once (stop_and_join sets EXTERNAL_STOP first)
                                    let until = std::time::Instant::now() + Duration::from_millis(wait);
                                    while std::time::Instant::now() < until && !search::EXTERNAL_STOP.load(Ordering::SeqCst) {
                                        std::thread::sleep(Duration::from_millis(2));
                                    }
                                }
                            }
                            match best {
                                Some(mv) => out!("bestmove {}", cozy_chess::util::display_uci_move(&search_board, mv)),
                                None => out!("bestmove 0000"),
                            }
                            let _ = io::stdout().flush();
                        })
                        .expect("failed to spawn search thread"),
                );
            }
            "stop" => {
                stop_and_join(&stop_flag, &mut search_thread);
            }
            "quit" => {
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
            | "bookpath"
    )
}
