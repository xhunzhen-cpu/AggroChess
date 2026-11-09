//! Skill Level 1..21 (strength limiter), in the spirit of Patricia's `Skill_Level`.
//!
//! Level 21 is the engine at full strength: nothing in this module runs, and the search is
//! exactly the unrestricted one. Levels 1..20 weaken the engine with four mechanisms:
//!
//! 1. **MultiPV candidates.** The root is searched for several candidate moves (MultiPV), so
//!    every candidate has a real score from the engine's own (personality-including) point of
//!    view.
//! 2. **Controlled search limits.** A node budget per move (and a depth cap) that grows with
//!    the level, so the weak levels see only short tactics and move quickly whatever the
//!    clock; the lowest levels also search more selectively (extra late-move reductions, no
//!    quiet checks in quiescence, no check extensions). The long-think extras (Tal's look,
//!    forcing-line priming, the rescue look, swindles) and Lazy SMP helper threads are off.
//! 3. **A centipawn-loss budget.** Every move adds `cp_loss` centipawns to the engine's
//!    allowance for mistakes (capped at `max_budget`), and the chosen move's loss against the
//!    best candidate is paid out of it (as in Patricia). The long-run average loss per move is
//!    therefore about `cp_loss`, delivered as a mix of best moves, inaccuracies and the
//!    occasional real mistake rather than random howlers.
//! 4. **Probabilistic, style-aware choice.** Each move draws a random target loss within the
//!    allowance and picks the candidate closest to it, tilted towards moves in the engine's
//!    character (sacrifices, checks and captures, moves at the enemy king) by an amount that
//!    depends only on the personality options. A weak AggroChess still plays like AggroChess.
//!
//! The Skill Level never changes the personality options, and the personality options never
//! change the strength limits.

use crate::search::MATE_BOUND;
use cozy_chess::{Board, Move, Piece};
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

/// Full strength.
pub const FULL: i32 = 21;
pub const MIN: i32 = 1;

/// The current Skill Level (1..=21).
pub static LEVEL: AtomicI32 = AtomicI32::new(FULL);
/// The mistake allowance accumulated in the current game (centipawns).
pub static BUDGET: AtomicI32 = AtomicI32::new(0);
/// Random state (xorshift64*); reseeded on `ucinewgame` unless a seed was fixed.
static RNG: AtomicU64 = AtomicU64::new(0x9E37_79B9_7F4A_7C15);
/// Fixed seed for reproducible tests (`AGGRO_SEED` env or the hidden `x_skillseed` option); 0 = time.
pub static FIXED_SEED: AtomicU64 = AtomicU64::new(0);

/// The parameters of one Skill Level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Level {
    /// candidate moves searched (MultiPV)
    pub multipv: usize,
    /// node budget per move (0 = no node limit, time only)
    pub nodes: u64,
    /// maximum iteration depth (0 = no limit)
    pub depth: i32,
    /// mistake allowance added per move, centipawns
    pub cp_loss: i32,
    /// cap on the accumulated allowance (bounds the largest single mistake)
    pub max_budget: i32,
    /// a loss this small is always allowed (variety between near-equal moves)
    pub slack: i32,
    /// share of the normal thinking time used (MultiPV costs depth; percent)
    pub time_pct: u32,
    /// extra late-move reduction (selective, "careless" search)
    pub lmr_add: i32,
    /// quiet checks in the quiescence search
    pub qs_checks: bool,
    /// check extensions
    pub check_ext: bool,
    /// the opening book is used for this many plies of the game (0 = no limit)
    pub book_plies: u32,
}

/// Parameters for levels 1..=20 (index level-1). Level 21 has no entry: no weakening at all.
///
/// The targets are behavioural (see the README for the measured ratings): 1 extremely weak,
/// 2-4 beginner, 5-7 lower intermediate, 8-10 intermediate, 11-13 upper intermediate, 14-16
/// advanced, 17-19 strong, 20 near full strength.
#[rustfmt::skip]
pub const TABLE: [Level; 20] = [
    //    pv  nodes     depth cp_loss max  slack time lmr  qs_chk  chk_ext book
    lvl(8,     150,     2,  200,  800,  20,  100, 2, false, false,  6),  // 1
    lvl(8,     220,     2,  170,  700,  16,  100, 2, false, false,  8),  // 2
    lvl(8,     300,     3,  140,  600,  14,  100, 2, false, true,  10),  // 3
    lvl(7,     410,     3,  115,  520,  12,  100, 1, true,  true,  12),  // 4
    lvl(7,     630,     4,   95,  450,  10,  100, 1, true,  true,  14),  // 5
    lvl(7,     980,     4,   80,  400,   9,  100, 1, true,  true,  16),  // 6
    lvl(6,    1500,     5,   65,  350,   8,  100, 0, true,  true,  18),  // 7
    lvl(6,    2350,     0,   55,  300,   7,  100, 0, true,  true,  20),  // 8
    lvl(6,    3600,     0,   46,  260,   6,  100, 0, true,  true,  24),  // 9
    lvl(5,    5600,     0,   39,  230,   6,  100, 0, true,  true,  28),  // 10
    lvl(5,    8700,     0,   32,  200,   5,  100, 0, true,  true,   0),  // 11
    lvl(5,   13400,     0,   27,  175,   5,  100, 0, true,  true,   0),  // 12
    lvl(5,   21000,     0,   22,  150,   4,  100, 0, true,  true,   0),  // 13
    lvl(5,   32000,     0,   18,  130,   4,  100, 0, true,  true,   0),  // 14
    lvl(5,   50000,     0,   15,  110,   4,  100, 0, true,  true,   0),  // 15
    lvl(5,   77000,     0,   12,   95,   3,  100, 0, true,  true,   0),  // 16
    lvl(4,  120000,     0,   10,   75,   3,   95, 0, true,  true,   0),  // 17
    lvl(4,  185000,     0,    8,   60,   3,   90, 0, true,  true,   0),  // 18
    lvl(3,       0,     0,    8,   60,   3,   90, 0, true,  true,   0),  // 19
    lvl(2,       0,     0,    5,   35,   2,   95, 0, true,  true,   0),  // 20
];

#[allow(clippy::too_many_arguments)]
const fn lvl(multipv: usize, nodes: u64, depth: i32, cp_loss: i32, max_budget: i32, slack: i32, time_pct: u32, lmr_add: i32, qs_checks: bool, check_ext: bool, book_plies: u32) -> Level {
    Level { multipv, nodes, depth, cp_loss, max_budget, slack, time_pct, lmr_add, qs_checks, check_ext, book_plies }
}

/// The current level.
pub fn level() -> i32 {
    LEVEL.load(Ordering::Relaxed).clamp(MIN, FULL)
}

/// Whether the engine is at full strength.
pub fn full_strength() -> bool {
    level() >= FULL
}

/// The parameters of the current level, or None at full strength.
pub fn params() -> Option<Level> {
    params_for(level())
}

pub fn params_for(level: i32) -> Option<Level> {
    if level >= FULL {
        None
    } else {
        Some(TABLE[(level.clamp(MIN, FULL - 1) - 1) as usize])
    }
}

/// Set the level (clamped to 1..=21). Starts a fresh allowance.
pub fn set_level(v: i32) {
    LEVEL.store(v.clamp(MIN, FULL), Ordering::Relaxed);
    BUDGET.store(0, Ordering::Relaxed);
}

/// A new game: reset the allowance and reseed the random choice.
pub fn new_game() {
    BUDGET.store(0, Ordering::Relaxed);
    reseed();
}

/// Reseed from the fixed seed (if any) or the clock.
pub fn reseed() {
    let fixed = FIXED_SEED.load(Ordering::Relaxed);
    let seed = if fixed != 0 {
        fixed
    } else {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64 ^ (std::process::id() as u64).rotate_left(32))
            .unwrap_or(0x2545_F491_4F6C_DD1D)
    };
    RNG.store(seed | 1, Ordering::Relaxed);
}

/// A random number (xorshift64*).
fn next_u64() -> u64 {
    let mut x = RNG.load(Ordering::Relaxed);
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    RNG.store(x, Ordering::Relaxed);
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

/// Uniform in 0..=n.
fn below_incl(n: i32) -> i32 {
    if n <= 0 {
        0
    } else {
        (next_u64() % (n as u64 + 1)) as i32
    }
}

/// One candidate line from the MultiPV search.
#[derive(Clone, Debug)]
pub struct Line {
    pub mv: Move,
    /// the search score (includes the sacrifice bonus, like the full-strength engine's choice)
    pub score: i32,
    pub pv: Vec<Move>,
    /// the sacrifice bonus inside `score` (0 if none)
    pub sb: i32,
    /// the root move earned the sacrifice bonus as a sacrifice (not just the forcing tie-break)
    pub sac: bool,
}

/// What the selection did, for the `info string` line.
#[derive(Clone, Copy, Debug)]
pub struct Choice {
    pub index: usize,
    pub loss: i32,
    pub allowance: i32,
    pub target: i32,
}

/// Material from `color`'s point of view (P100 N320 B330 R500 Q900).
fn material(board: &Board, color: cozy_chess::Color) -> i32 {
    const V: [i32; 6] = [100, 320, 330, 500, 900, 0];
    let mut s = 0;
    for p in Piece::ALL {
        s += V[p as usize] * (board.colored_pieces(color, p).len() as i32 - board.colored_pieces(!color, p).len() as i32);
    }
    s
}

/// The personality tilt of a candidate (centipawns), from the personality options only.
fn style_bonus(board: &Board, line: &Line) -> i32 {
    let p = crate::personality::current();
    let stm = board.side_to_move();
    let m = line.mv;
    let mut child = board.clone();
    child.play_unchecked(m);
    let gives_check = !child.checkers().is_empty();
    let capture = board.colors(!stm).has(m.to);
    let ezone = crate::eval::king_zone(board, !stm);
    let at_king = gives_check || ezone & m.to.bitboard().0 != 0;
    // a sacrifice: the root move earned the sacrifice bonus, or we are at least a piece-ish
    // down two plies into the line (our move, their reply, our next move played)
    let mut sac = line.sac;
    if !sac {
        let before = material(board, stm);
        let mut b = board.clone();
        for &mv in line.pv.iter().take(3) {
            if !b.is_legal(mv) {
                break;
            }
            b.play_unchecked(mv);
        }
        sac = before - material(&b, stm) >= 150;
    }
    let mut bonus = 0;
    if sac {
        bonus += 40 * p.sacrifice_appetite / 100;
    }
    if gives_check || capture {
        bonus += 12 * p.aggression / 100;
    }
    if at_king {
        bonus += 12 * p.king_attack / 100;
    }
    bonus
}

/// Choose a move from the candidate lines (sorted best first) and pay for it out of the
/// allowance. Returns the chosen index.
pub fn choose(board: &Board, lines: &[Line], lvl: &Level) -> Choice {
    let best = lines[0].score;
    let budget = BUDGET.load(Ordering::Relaxed).clamp(0, lvl.max_budget);
    let allowance = budget + lvl.slack;
    let target = below_incl(allowance);
    let mut pick = 0;
    let mut pick_val = i32::MIN;
    for (i, l) in lines.iter().enumerate() {
        let loss = (best - l.score).max(0);
        if loss > allowance {
            continue;
        }
        // never walk into a mate the best line avoids
        if l.score <= -MATE_BOUND && best > -MATE_BOUND {
            continue;
        }
        // a forced mate of our own is never thrown away
        if best >= MATE_BOUND && l.score < MATE_BOUND {
            continue;
        }
        let val = -(loss - target).abs() + style_bonus(board, l);
        if val > pick_val {
            pick = i;
            pick_val = val;
        }
    }
    let loss = (best - lines[pick].score).max(0);
    let left = (budget - loss + lvl.cp_loss).clamp(0, lvl.max_budget);
    BUDGET.store(left, Ordering::Relaxed);
    Choice { index: pick, loss, allowance, target }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_monotone() {
        let _lock = crate::test_lock();
        for i in 1..TABLE.len() {
            let (a, b) = (TABLE[i - 1], TABLE[i]);
            assert!(b.cp_loss <= a.cp_loss, "cp_loss at level {}", i + 1);
            assert!(b.max_budget <= a.max_budget, "max_budget at level {}", i + 1);
            let na = if a.nodes == 0 { u64::MAX } else { a.nodes };
            let nb = if b.nodes == 0 { u64::MAX } else { b.nodes };
            assert!(nb >= na, "nodes at level {}", i + 1);
            let da = if a.depth == 0 { i32::MAX } else { a.depth };
            let db = if b.depth == 0 { i32::MAX } else { b.depth };
            assert!(db >= da, "depth at level {}", i + 1);
            assert!(b.lmr_add <= a.lmr_add);
            assert!(b.multipv >= 2);
        }
        assert!(params_for(FULL).is_none());
        assert!(params_for(20).is_some());
    }

    #[test]
    fn choice_respects_the_allowance_and_mates() {
        let _lock = crate::test_lock();
        let b = Board::default();
        let mut moves = Vec::new();
        b.generate_moves(|m| {
            moves.extend(m);
            false
        });
        let line = |i: usize, score: i32| Line { mv: moves[i], score, pv: vec![moves[i]], sb: 0, sac: false };
        let lines = vec![line(0, 50), line(1, 40), line(2, -400), line(3, -900)];
        let lvl = TABLE[0];
        FIXED_SEED.store(12345, Ordering::Relaxed);
        reseed();
        for _ in 0..200 {
            BUDGET.store(0, Ordering::Relaxed);
            let c = choose(&b, &lines, &lvl);
            // with an empty allowance only moves within the slack (+ style) are possible
            assert!(c.loss <= lvl.slack, "loss {} allowance {}", c.loss, c.allowance);
        }
        // a forced mate is always kept
        let mates = vec![line(0, MATE_BOUND + 10), line(1, 300)];
        for _ in 0..50 {
            BUDGET.store(lvl.max_budget, Ordering::Relaxed);
            assert_eq!(choose(&b, &mates, &lvl).index, 0);
        }
        // the average loss per move stays near cp_loss
        BUDGET.store(0, Ordering::Relaxed);
        let spread: Vec<Line> = (0..8).map(|i| line(i, 100 - 40 * i as i32)).collect();
        let mut total = 0;
        let n = 400;
        for _ in 0..n {
            total += choose(&b, &spread, &lvl).loss;
        }
        let avg = total / n;
        assert!(avg <= lvl.cp_loss + lvl.slack, "average loss {} vs cp_loss {}", avg, lvl.cp_loss);
        assert!(avg >= lvl.cp_loss / 3, "average loss {} too small for cp_loss {}", avg, lvl.cp_loss);
        FIXED_SEED.store(0, Ordering::Relaxed);
    }
}
