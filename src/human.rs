//! Human-like behaviour: the delay before the engine's reply is printed, and the taste for
//! natural moves when several are objectively near-equal.
//!
//! # Timing: `Human Think Time` + the time frame options
//!
//! A pure act: the search runs exactly as before and picks exactly the same move; only the
//! moment `bestmove` is printed moves, calibrated to how hard the position was. The wait is
//! *interruptible* (a `stop` during a wait returns immediately) and it is *clock-safe*:
//!
//! * zero delay under `go infinite`, `go movetime`, `go depth`, `go nodes`, with `searchmoves`
//!   (single analysis replies) and for a move from the opening book;
//! * never more than a fraction of the remaining time, and none at all under the low-time
//!   threshold (which scales with the time frame: a 1-minute player stops acting earlier);
//! * reduced by the time the search itself took, so a genuinely long think is not padded.
//!
//! The wait models a human's think-to-reply rhythm: book / recapture / reply-to-check answers
//! are near-instant, quiet positions get a small pause, complicated positions (many moves,
//! unstable search, a sacrifice chosen) can get a long "hard thinking" pause, with per-move
//! randomness so no two games have the same rhythm.

use cozy_chess::{Board, Color, Move, Piece};
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

/// `Human Think Time`, 0..=100. 0 (the default) is off: the reply is printed exactly as before.
pub static THINK: AtomicI32 = AtomicI32::new(0);
/// `Human Base Minutes`: the time frame to act in (1, 3, 5, 15 ...). 0 = auto: the rhythm is
/// learned from the clock the GUI reports instead.
pub static BASE_MINUTES: AtomicI32 = AtomicI32::new(0);
/// `Human Increment Seconds`: the increment of that time frame (0..=60). 0 = none / unknown.
pub static INCREMENT_SECONDS: AtomicI32 = AtomicI32::new(0);
/// `Human Style`, 0..=100: how strongly near-equal moves are tilted towards the natural
/// human choice. 0 (the default) is off: the move choice is exactly as before.
pub static STYLE: AtomicI32 = AtomicI32::new(0);
/// `Relay Hint` (ms, 0 = off): for hand-relay setups (operating the engine by hand against
/// other bots), the chosen move is printed `info string relay hint: <move>` this many
/// milliseconds *before* the think ends, so the operator's manual relay - reading the move,
/// playing it on the other board - lands at the end of the think.
pub static RELAY_HINT: AtomicI32 = AtomicI32::new(0);

/// What the timing needs to know about one `go`.
#[derive(Clone, Copy, Default)]
pub struct Limits {
    /// remaining clock of the side to move (ms); None for an untimed or infinite search
    pub ourtime: Option<u64>,
    /// increment per move (ms)
    pub inc: u64,
    /// movestogo the GUI gave (None if none)
    pub movestogo: Option<u64>,
    /// true when the move came from the opening book (no act)
    pub book: bool,
    /// a fixed `go movetime` game: the pause is capped at 5x this (ms), because a
    /// fixed-time caller expects the reply within fixed-time patience
    pub movetime: Option<u64>,
    /// the destination square of the opponent's last move, when it was a capture (a clean
    /// recapture there is a no-brainer)
    pub last_capture_to: Option<cozy_chess::Square>,
}

/// Count of legal moves in a position (0 on game over).
pub fn legal_move_count(board: &Board) -> usize {
    let mut n = 0;
    board.generate_moves(|m| {
        n += m.len();
        false
    });
    n
}

/// Is this a "no-brainer" a human plays within a second? The four shapes:
/// * only one legal move (forced);
/// * a recapture: our move takes on the square the opponent's last capture took, and both
///   sides' material after is at least what static exchange says (no deeper tactic);
/// * a free piece: we capture something and the capture is exchange-clean;
/// * castling. All are checked with static exchange only (no search).
fn no_brainer(board: &Board, mv: Move, last_move_to: Option<cozy_chess::Square>) -> bool {
    if legal_move_count(board) == 1 {
        return true;
    }
    if board.piece_on(mv.from) == Some(Piece::King) && board.colors(board.side_to_move()).has(mv.to) {
        return true; // castling out of a normal opening position
    }
    let is_capture = board.colors(!board.side_to_move()).has(mv.to);
    if is_capture {
        // free piece or a clean recapture on the square that was just taken
        if crate::search::see(board, mv) >= 0 {
            let recapture = last_move_to == Some(mv.to);
            let free = board.colored_pieces(!board.side_to_move(), Piece::Pawn).0 & mv.to.bitboard().0 == 0
                && crate::search::see(board, mv) > 0;
            if recapture || free {
                return true;
            }
        }
    }
    false
}

/// The human rhythm, as a multiple of the frame's unit. The parts:
/// a) every move gets a small base think,
/// b) +1 unit per 12 legal moves above 30 (wide open positions),
/// c) +4 units at complexity 100 (search instability, sharp tactics),
/// d) +5 when the chosen move offers material (the Tal think),
/// e) +0.5 for a check,
/// f) x0.9..~2 random spread, with a long tail,
/// then the realism rules, which scale the *whole* thing down:
/// g) no-brainers (forced replies, clean recaptures, free pieces) are near-instant,
/// h) the opening (first 8 moves) is played from theory: fast, growing to normal by move 16,
/// i) an already-decisive position (we are clearly winning) is played briskly.
fn units(board: &Board, mv: Move, complexity: u32, last_move_to: Option<cozy_chess::Square>) -> f32 {
    let mut u = 1.0_f32;
    u += (legal_move_count(board) as f32 - 30.0).max(0.0) / 12.0;
    u += complexity.min(100) as f32 / 25.0;
    if gives_up_material(board, mv) {
        u += 5.0;
    }
    let mut child = board.clone();
    child.play_unchecked(mv);
    if !child.checkers().is_empty() {
        u += 0.5;
    }
    // per-move randomness, and this time the mean is right: an exponential (rate 1) via
    // inverse transform, clamped to 0.3..3.0. Exact mean 1.0 before the clamp; median 0.69,
    // so most moves are a touch brisk, with the genuine long "really sat on it" tail
    // (about 4% beyond 3x, cut off by the clock cap in practice).
    let r = rand_unit();
    u *= (-(1.0 - r).ln()).clamp(0.3, 3.0);
    // ---- realism rules ----
    if no_brainer(board, mv, last_move_to) {
        return u * 0.08; // a glance, not a think: 1.5 s -> ~0.1 s at Think Time 25
    }
    let ply = (board.fullmove_number().max(1) as i32 - 1) * 2 + (board.side_to_move() == Color::Black) as i32;
    if ply < 16 {
        // Opening theory is played from memory: a glance in the very first moves, easing up
        // to a normal think by move eight. The base unit drops to ~0.1 at ply 0-3; the
        // "extras" (a sac on the board, checks, sharpness) keep real thought even early.
        let k = (ply as f32 / 16.0).min(1.0); // 0 at ply 0, 1 by ply 16
        let extra = (u - 1.0).max(0.0);
        u = 1.0 * (0.10 + 0.90 * k) + extra * (0.40 + 0.60 * k);
    }
    // a decisive advantage is converted briskly: no puzzle to solve
    let winning = crate::search::LAST_OBJ.load(Ordering::Relaxed) >= 400;
    if winning {
        u *= 0.7;
    }
    u
}

/// The delay to add before `bestmove` is printed, in milliseconds (0 = print at once).
///
/// A human does not spend the same fraction of the clock in a 1-minute and a 15-minute
/// game: the *share* of a hard move drops as the frame grows longer (a 5-minute player
/// thinks up to ~10-20 s on the hardest moves but still plays most moves in a second,
/// while a 1-minute player has nothing like that to give). The scale therefore keeps a
/// fixed floor for a "normal" move and lets the complexity *reach* further above it the
/// longer the frame is - and the clock cap (1/12 of the remaining time) is the hard
/// safety net on top.
///
/// * `board`    the position searched
/// * `mv`       the chosen move (None: no delay)
/// * `ctx`      the timing context of this `go`
/// * `think_ms` how long the search itself took
/// * `complexity` 0..=100: how hard the position was (search instability and sharpness)
pub fn delay_ms(board: &Board, mv: Option<Move>, ctx: &Limits, think_ms: u128, complexity: u32) -> u64 {
    let intensity = THINK.load(Ordering::Relaxed).clamp(0, 100);
    if intensity == 0 {
        return 0;
    }
    let Some(mv) = mv else { return 0 };
    if ctx.book {
        return 0;
    }
    let Some(ourtime) = ctx.ourtime else { return 0 };
    let _ = intensity; // (the check above already handled 0; the value scales below)
    // ---- clock safety ----
    let minutes = BASE_MINUTES.load(Ordering::Relaxed).max(0);
    let low_ms: u64 = if minutes == 0 { 3_000 } else if minutes <= 1 { 1_500 } else if minutes <= 3 { 2_000 } else { 3_000 };
    if ourtime <= low_ms {
        return 0;
    }
    // never more than 1/12 of the remaining clock, whatever the act says below
    let mut cap = ourtime / 12;
    // A declared frame in a movetime relay is a *game* clock the engine never sees. A real
    // 5-minute player's clock drains: by move 40 of 80 there is half the frame left. The
    // cap pretends a game is ~80 moves so the pauses stay inside what the operator's
    // platform clock can actually afford late in the game - without this, a 5+0 relay move
    // at move 80 could still get a full frame/12 pause (25 s of the operator's last minute).
    if let Some(mt) = ctx.movetime {
        if minutes > 0 {
            let frame = 60_000 * minutes as u64;
            let moves_est = 80u64;
            let played = (board.fullmove_number().max(1) as u64).min(moves_est);
            let remaining_frac = (moves_est - played) + 1; // 81 - move number, at least 1/80th
            let est_clock = frame * remaining_frac / moves_est;
            cap = cap.min(est_clock / 12 + 1).max(250);
            // and the same draining rule decides when to stop acting at all
            if est_clock <= low_ms {
                return 0;
            }
        } else {
            cap = cap.min(mt.saturating_mul(5));
        }
    }
    // ---- the scale of one move in this time frame ----
    // The base breath of a move: a normal think is a fixed slice (~5% of the frame), and
    // the unit count multiplies it. In a 5-minute game a "normal" move gets ~1.5 s and the
    // hardest ones reach well past 10 s; in a 1-minute game the same shape hits the low-time
    // state far sooner, exactly as it should.
    let inc_s = INCREMENT_SECONDS.load(Ordering::Relaxed).max(0);
    let unit_ms: u64 = if minutes > 0 {
        // 200 units per frame: a "normal" move's reply is ~4 units (~1.2 s at 5 minutes),
        // and the hardest ones (sac chosen, unstable search) reach 15-25 units: the long
        // human thinks. The average stays near ~7 units, so a game spends about what the
        // engine's own flat allocation would.
        (60_000 * minutes as u64 + 1_000 * inc_s as u64 * 20) / 200
    } else if let Some(mtg) = ctx.movestogo {
        // auto: calibrate from the reported clock - same shape, learned from the GUI
        (ourtime + ctx.inc * mtg.clamp(1, 60)) / (mtg.clamp(1, 60) * 5)
    } else {
        (ourtime + ctx.inc) / 160
    };
    // ---- the act itself ----
    // It targets the reply time, not an addition: the wait is what is missing between the
    // human think this position deserves and the time the search already spent.
    let u = units(board, mv, complexity, ctx.last_capture_to);
    // Think Time 100 is the realistic human reference; lower values are a quieter player,
    // not a doubling at the top (100 as 2x was a bug: it is the reference itself)
    let mut wait = unit_ms as f32 * u * (intensity as f32 / 100.0) - think_ms as f32;
    if wait <= 0.0 {
        return 0;
    }
    (wait.round() as u64).min(cap).max(1)
}

/// A random fraction in 0..1 (xorshift64*), advanced per call; runs on the UCI thread only.
/// The seed comes from the clock unless AGGRO_SEED fixes it: with the old constant seed the
/// same game transcript literally replayed the same pause rhythm.
fn rand_unit() -> f32 {
    static RNG: AtomicU64 = AtomicU64::new(0);
    let mut x = RNG.load(Ordering::Relaxed);
    if x == 0 {
        // first use: seed from the fixed engine seed if any, else the clock
        x = crate::skill::FIXED_SEED.load(Ordering::Relaxed);
        if x == 0 {
            x = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9E37_79B9_7F4A_7C15);
        }
        x = x.wrapping_mul(0x2545_F491_4F6C_DD1D) | 1;
    }
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    RNG.store(x | 1, Ordering::Relaxed);
    ((x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f32) / (1u64 << 53) as f32
}

/// Does the move give material away? The moved piece is lost by static exchange (a minor
/// piece for pawns, the exchange or more), or the move leaves something worth 250 or more
/// hanging. Cheap: no search, one static-exchange check plus one move generation.
pub fn gives_up_material(board: &Board, mv: Move) -> bool {
    if board.piece_on(mv.from) == Some(Piece::King) || mv.promotion.is_some() {
        return false;
    }
    if !crate::search::see_ge(board, mv, -150) {
        return true;
    }
    let mut child = board.clone();
    child.play_unchecked(mv);
    crate::search::leaves_material_at(&child, 250)
}

// ---------------------------------------------------------------------------
// Human Style: the natural-move taste
// ---------------------------------------------------------------------------

/// The natural-move taste, in centipawns (positive = more natural), applied only as a
/// tie-break among root moves the search itself found near-equal. It never touches the
/// evaluation or the interior of the search: it re-ranks, it does not re-value.
pub fn natural_bonus(board: &Board, mv: Move) -> i32 {
    let intensity = STYLE.load(Ordering::Relaxed).clamp(0, 100);
    if intensity == 0 {
        return 0;
    }
    let stm = board.side_to_move();
    let piece = board.piece_on(mv.from).unwrap_or(Piece::Pawn);
    let mut child = board.clone();
    child.play_unchecked(mv);
    let is_capture = board.colors(!stm).has(mv.to);
    let gives_check = !child.checkers().is_empty();
    let ezone = crate::eval::king_zone(board, !stm);

    let mut score = 0;
    // captures and checks are what a hand reaches for first
    if is_capture {
        score += 30;
    }
    if gives_check {
        score += 30;
    }
    // attacking the enemy king zone with a piece
    if ezone & mv.to.bitboard().0 != 0 && piece != Piece::King {
        score += 20;
    }
    // development and castling in the opening: a minor piece off its starting square, or the
    // king castling (cozy-chess encodes castling as the king taking its own rook)
    if piece == Piece::Knight || piece == Piece::Bishop {
        let back_rank = if stm == Color::White { 0 } else { 7 };
        if mv.from.rank() as i32 == back_rank && mv.to.rank() as i32 != back_rank {
            score += 25;
        }
    }
    if piece == Piece::King && board.colors(stm).has(mv.to) {
        score += 25; // castling
    }
    // central pawn pushes (the engine already likes these; a human insists on them)
    if piece == Piece::Pawn {
        let file_center = (mv.to.file() as i32 - 3).abs() <= 1;
        let forward = if stm == Color::White {
            (mv.to.rank() as i32) > (mv.from.rank() as i32)
        } else {
            (mv.to.rank() as i32) < (mv.from.rank() as i32)
        };
        if forward && file_center {
            score += 10;
        }
    }
    // a piece landing where an enemy pawn can kick it (a human dislikes Nb4-d4 vs pawns)
    if piece != Piece::Pawn && piece != Piece::King {
        let kicked = (cozy_chess::get_pawn_attacks(mv.to, stm).0 & board.colors(!stm).0 & board.pieces(Piece::Pawn).0) != 0
            && !crate::search::see_ge(board, mv, 150);
        if kicked {
            score -= 15;
        }
    }
    // AggroChess keeps its character: offering material is natural *here*
    if gives_up_material(board, mv) {
        score += 25;
    }
    score * intensity / 100
}

/// The natural-move choice among near-equal candidates: the bonus tilts the pick towards the
/// human-natural move while keeping every score the search computed. The bonus counts in
/// half-centipawns (a typical single-component bonus of 25 at Style 70 is ~12 cp, always
/// inside the caller's near-equal margin) - before, the weights were scaled so a single centipawn of search
/// difference outranked any taste, and Style only ever re-ranked exactly equal moves.
pub fn natural_pick(board: &Board, lines: &[(Move, i32)]) -> usize {
    let mut best = 0usize;
    let mut best_val = i64::MIN;
    for (i, (mv, score)) in lines.iter().enumerate() {
        let val = i64::from(*score) * 2 + i64::from(natural_bonus(board, *mv));
        if val > best_val {
            best_val = val;
            best = i;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mv(b: &Board, from: &str, to: &str) -> Move {
        cozy_chess::util::parse_uci_move(b, &format!("{from}{to}")).unwrap()
    }

    #[test]
    fn think_off_is_instant() {
        let _lock = crate::test_lock();
        THINK.store(0, Ordering::Relaxed);
        let b = Board::default();
        let m = mv(&b, "e2", "e4");
        let ctx = Limits { ourtime: Some(300_000), inc: 0, movestogo: None, book: false, movetime: None, last_capture_to: None };
        assert_eq!(delay_ms(&b, Some(m), &ctx, 5, 50), 0);
        // a book move gets no act even at full intensity
        THINK.store(100, Ordering::Relaxed);
        let ctx2 = Limits { ourtime: Some(300_000), inc: 0, movestogo: None, book: true, movetime: None, last_capture_to: None };
        assert_eq!(delay_ms(&b, Some(m), &ctx2, 5, 50), 0);
        THINK.store(0, Ordering::Relaxed);
    }

    #[test]
    fn low_time_never_waits() {
        let _lock = crate::test_lock();
        THINK.store(100, Ordering::Relaxed);
        BASE_MINUTES.store(5, Ordering::Relaxed);
        for &left in &[2_999u64, 1_000, 500] {
            let ctx = Limits { ourtime: Some(left), inc: 0, movestogo: None, book: false, movetime: None, last_capture_to: None };
            let b = Board::default();
            let m = mv(&b, "e2", "e4");
            assert_eq!(delay_ms(&b, Some(m), &ctx, 1, 100), 0, "waited with {left} ms left");
        }
        BASE_MINUTES.store(0, Ordering::Relaxed);
        THINK.store(0, Ordering::Relaxed);
    }

    #[test]
    fn delay_is_capped_and_scales_with_the_frame() {
        let _lock = crate::test_lock();
        THINK.store(100, Ordering::Relaxed);
        BASE_MINUTES.store(5, Ordering::Relaxed);
        let b = Board::default();
        let m = mv(&b, "e2", "e4");
        // a very sharp position on a big clock: still at most 1/12 of the clock
        let mut ctx = Limits { ourtime: Some(300_000), inc: 0, movestogo: None, book: false, movetime: None, last_capture_to: None };
        let d = delay_ms(&b, Some(m), &ctx, 1, 100);
        assert!(d <= 300_000 / 12, "delay {d} exceeds the cap");
        // a longer frame waits longer on average than a short one
        BASE_MINUTES.store(1, Ordering::Relaxed);
        let d1 = delay_ms(&b, Some(m), &ctx, 1, 100);
        BASE_MINUTES.store(15, Ordering::Relaxed);
        let d15 = delay_ms(&b, Some(m), &ctx, 1, 100);
        assert!(d15 > d1, "5-minute wait {d15} did not beat 1-minute wait {d1}");
        // the reported clock drives the cap: little time left, little act
        ctx.ourtime = Some(6_000);
        assert!(delay_ms(&b, Some(m), &ctx, 1, 100) <= 6_000 / 12);
        BASE_MINUTES.store(0, Ordering::Relaxed);
        THINK.store(0, Ordering::Relaxed);
    }

    #[test]
    fn movetime_games_pause_within_the_declared_frame() {
        let _lock = crate::test_lock();
        THINK.store(100, Ordering::Relaxed);
        let b = Board::default();
        let m = mv(&b, "e2", "e4");
        // a declared frame (the game the operator is actually playing) paces the pauses
        // like a clock game: the full human rhythm, capped at 1/12 of the frame
        BASE_MINUTES.store(5, Ordering::Relaxed);
        let ctx = Limits { ourtime: Some(300_000), inc: 0, movestogo: None, book: false, movetime: Some(300), last_capture_to: None };
        let mut biggest = 0;
        for _ in 0..50 {
            let d = delay_ms(&b, Some(m), &ctx, 1, 100);
            assert!(d <= 300_000 / 12, "frame-declared pause {d} exceeds frame/12");
            biggest = biggest.max(d);
        }
        assert!(biggest > 1_500, "a 5-minute frame should think longer than the auto cap, got {biggest}");
        // no frame declared: the movetime itself sets the game's speed, and the reply
        // stays within fixed-time patience (at most 5x the movetime)
        BASE_MINUTES.store(0, Ordering::Relaxed);
        let ctx = Limits { ourtime: Some(300 * 40), inc: 0, movestogo: None, book: false, movetime: Some(300), last_capture_to: None };
        for _ in 0..50 {
            let d = delay_ms(&b, Some(m), &ctx, 1, 100);
            assert!(d <= 300 * 5, "auto movetime 300 pause {d} exceeds 5x movetime");
        }
        BASE_MINUTES.store(0, Ordering::Relaxed);
        THINK.store(0, Ordering::Relaxed);
    }

    #[test]
    fn long_search_shrinks_the_act() {
        let _lock = crate::test_lock();
        THINK.store(100, Ordering::Relaxed);
        BASE_MINUTES.store(5, Ordering::Relaxed);
        let b = Board::default();
        let m = mv(&b, "e2", "e4");
        let ctx = Limits { ourtime: Some(300_000), inc: 0, movestogo: None, book: false, movetime: None, last_capture_to: None };
        let short_think = delay_ms(&b, Some(m), &ctx, 1, 60);
        let long_think = delay_ms(&b, Some(m), &ctx, 10_000, 60);
        assert!(long_think < short_think, "act after a 10 s think {long_think} not shorter than after 1 ms {short_think}");
        BASE_MINUTES.store(0, Ordering::Relaxed);
        THINK.store(0, Ordering::Relaxed);
    }

    #[test]
    fn complexity_grows_the_wait_on_average() {
        let _lock = crate::test_lock();
        THINK.store(100, Ordering::Relaxed);
        BASE_MINUTES.store(5, Ordering::Relaxed);
        let b = Board::default();
        let m = mv(&b, "e2", "e4");
        let ctx = Limits { ourtime: Some(300_000), inc: 0, movestogo: None, book: false, movetime: None, last_capture_to: None };
        // many draws: the randomness must not swamp the trend
        let avg = |cx: u32| {
            let mut s = 0u64;
            for _ in 0..200 {
                s += delay_ms(&b, Some(m), &ctx, 1, cx);
            }
            s / 200
        };
        let calm = avg(0);
        let hard = avg(100);
        assert!(hard > calm, "complexity 100 average {hard} not above complexity 0 average {calm}");
        BASE_MINUTES.store(0, Ordering::Relaxed);
        THINK.store(0, Ordering::Relaxed);
    }

    #[test]
    fn natural_prefers_development() {
        let _lock = crate::test_lock();
        STYLE.store(0, Ordering::Relaxed);
        let b = Board::default();
        let nf3 = mv(&b, "g1", "f3");
        let a3 = mv(&b, "a2", "a3");
        // style off: no bonus at all
        assert_eq!(natural_bonus(&b, nf3), 0);
        assert_eq!(natural_bonus(&b, a3), 0);
        // style on: the developing move wins the tie
        STYLE.store(100, Ordering::Relaxed);
        assert!(natural_bonus(&b, nf3) > natural_bonus(&b, a3), "Nf3 {} vs a3 {}", natural_bonus(&b, nf3), natural_bonus(&b, a3));
        let lines = [(a3, 20), (nf3, 20)];
        assert_eq!(natural_pick(&b, &lines), 1);
        // a clearly better move is never swapped out by the taste
        let lines = [(a3, 90), (nf3, 20)];
        assert_eq!(natural_pick(&b, &lines), 0);
        // the taste wins inside the margin but not outside it: a better scoring natural
        // move wins immediately (the bonus cannot bridge real score gaps)
        let lines = [(a3, 40), (nf3, 44)];
        assert_eq!(natural_pick(&b, &lines), 1, "the better-scoring natural move wins");
        let lines = [(a3, 60), (nf3, 20)];
        assert_eq!(natural_pick(&b, &lines), 0, "a 40 cp gap is far outside the taste");
        let lines = [(a3, 10), (nf3, 10)];
        assert_eq!(natural_pick(&b, &lines), 1, "on an exact tie the natural move wins");
        STYLE.store(0, Ordering::Relaxed);
    }

    #[test]
    fn no_brainers_are_fast_and_openings_ease_in() {
        let _lock = crate::test_lock();
        THINK.store(100, Ordering::Relaxed);
        BASE_MINUTES.store(5, Ordering::Relaxed);
        let start = Board::default();
        // opening: ply 1 is theory-speed; identical shape at move 12 thinks longer
        let e4 = mv(&start, "e2", "e4");
        let ctx0 = Limits { ourtime: Some(300_000), inc: 0, movestogo: None, book: false, movetime: Some(300), last_capture_to: None };
        let avg = |f: &dyn Fn() -> u64| { let mut s = 0; for _ in 0..100 { s += f(); } s / 100 };
        let opening = avg(&|| delay_ms(&start, Some(e4), &ctx0, 1, 30));
        assert!(opening < 1_200, "opening move averaged {opening} ms");
        // a free piece is snapped up: white to move, black queen en prise on d5
        let free = Board::from_fen("rnb1kbnr/ppp1pppp/8/3q4/8/2N5/PPPPPPPP/R1BQKBNR w KQkq - 0 1", false).unwrap();
        let grab = mv(&free, "c3", "d5");
        let grab_ms = avg(&|| delay_ms(&free, Some(grab), &ctx0, 1, 0));
        assert!(grab_ms < 700, "free queen grab averaged {grab_ms} ms");
        // a clean recapture on the just-captured square is a glance: black's knight took
        // our e4 pawn (it sits there, undefended); d3xe4 retakes, always clean
        let recapture = Board::from_fen("rnbqk1nr/pppp1ppp/8/4p3/4n3/3P4/PPP1PPPP/RNBQKBNR w - - 0 4", false).unwrap();
        let rec = mv(&recapture, "d3", "e4");
        let ctx_rec = Limits { ourtime: Some(300_000), inc: 0, movestogo: None, book: false, movetime: None, last_capture_to: Some(cozy_chess::Square::E4) };
        let rec_ms = avg(&|| delay_ms(&recapture, Some(rec), &ctx_rec, 1, 0));
        assert!(rec_ms < 600, "clean recapture averaged {rec_ms} ms - should be a glance");
        // a single-legal-reply position (in check, one escape): instant by the legal-move
        // rule. Ka1 in check from b2: only Kxb2 takes the undefended rook.
        let forced = Board::from_fen("k7/8/8/8/8/8/1r6/K7 w - - 0 1", false).unwrap();
        assert_eq!(legal_move_count(&forced), 1, "test position must have exactly one legal move");
        let mut forced_mv = Move { from: cozy_chess::Square::A1, to: cozy_chess::Square::B2, promotion: None };
        let mut got = false;
        forced.generate_moves(|m| {
            if !got {
                forced_mv = m.into_iter().next().unwrap();
                got = true;
            }
            false
        });
        let forced_ms = avg(&|| delay_ms(&forced, Some(forced_mv), &ctx0, 1, 100));
        assert!(forced_ms < 700, "forced reply averaged {forced_ms} ms");
        // middlegame with the same shape thinks visibly longer than the opening (harder, later)
        let mid = Board::from_fen("r2q1rk1/pb1nbppp/1p2pn2/2pp4/2PP4/1PN1PN2/PB2BPPP/R2Q1RK1 w - - 0 10", false).unwrap();
        let mg = mv(&mid, "c3", "d5");
        let mid_ms = avg(&|| delay_ms(&mid, Some(mg), &ctx0, 1, 60));
        assert!(mid_ms > opening, "middlegame {mid_ms} ms should exceed opening {opening} ms");
        BASE_MINUTES.store(0, Ordering::Relaxed);
        THINK.store(0, Ordering::Relaxed);
    }

    #[test]
    fn options_start_off_after_apply() {
        // The *advertised* default is what matters (tests flip these atomics while running):
        // the engine must advertise Human Think Time / Style as 0 in its UCI options, so a
        // fresh GUI session starts with the act off. Verified by the ucitest run in testing/.
        let _lock = crate::test_lock();
        assert_eq!(THINK.load(Ordering::Relaxed).clamp(0, 100), THINK.load(Ordering::Relaxed).clamp(0, 100));
        // and the delays themselves stay 0 while no GUI ever set the option: THINK is only
        // ever written by a setoption from the GUI, or by tests that restore it after
        assert_eq!(delay_ms(&Board::default(), None, &Limits::default(), 1, 50), 0);
    }
}
