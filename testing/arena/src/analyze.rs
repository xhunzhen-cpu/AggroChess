//! `analyze`: detect and classify sacrifices made by one engine in match JSONL output.
//!
//! Conventions (see testing/README.md, section "analyze"):
//! * Material values P=100 N=320 B=330 R=500 Q=900 (kings ignored).
//! * `bal(i)` = material of the analysed engine minus the opponent's in the position
//!   *before* ply `i` (ply 0 = first move of the game). Indices past the end of the
//!   game are clamped to the final position.
//! * Only plies played by the analysed engine and not taken from the opening file can
//!   start a sacrifice; engine book moves are analysed and flagged `book`.
//! * Scores in the JSONL are from the mover's view; Stockfish scores are from the side
//!   to move in the evaluated position and are normalised to the engine's view here.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use cozy_chess::{Board, Color, Piece};

use crate::engine::{is_mate_score, MATE_SCORE};
use crate::game::{self, GameRecord};
use crate::json::Json;
use crate::san;
use crate::sfeval::{self, EvalJob, SfSettings};
use crate::sys;

// ---------------------------------------------------------------- parameters

/// Piece values in the order P, N, B, R, Q.
pub const VALUES: [i32; 5] = [100, 320, 330, 500, 900];
const PIECES: [Piece; 5] = [Piece::Pawn, Piece::Knight, Piece::Bishop, Piece::Rook, Piece::Queen];
const PIECE_CH: [&str; 5] = ["P", "N", "B", "R", "Q"];

pub const QUEEN_D2: i32 = 300;
pub const QUEEN_D4: i32 = 300;
pub const EXCH_D2: i32 = 120;
pub const EXCH_D4: i32 = 120;
pub const PIECE_D2: i32 = 200;
pub const PIECE_D4: i32 = 150;
pub const PAWN_D: i32 = 100; // d2, d4 and d6
/// Sacrifice if the engine's score one move later is >= e0 - INTENT_DROP.
pub const INTENT_DROP: i32 = 150;
pub const MATE_WINDOW: usize = 30;
pub const RECOUP_WINDOW: usize = 24;
pub const RECOUP_MARGIN: i32 = 50;
pub const SUCCESS_SCORE_MARGIN: i32 = 50;
pub const SF_LATER_OFFSET: usize = 12;
pub const SF_SUCCESS: i32 = 150;
pub const UNSOUND_DROP: i32 = 150;
pub const UNSOUND_ABS: i32 = -100;
pub const PRACTICAL_SWING: i32 = 100;
pub const PRACTICAL_WINDOW: usize = 30;
/// Stockfish mates are mapped to ±SF_MATE (and cp scores clamped to that range).
pub const SF_MATE: i32 = 3000;
/// Trade guards (addition to the spec, so that trades interrupted by an in-between
/// move are excluded like ordinary recaptures; `--no-trade-guards` disables both):
/// (a) a candidate is rejected if bal(k+5) and bal(k+6) are both back to within this
///     margin of the baseline — the engine completed a delayed recapture with its own
///     move at k+4 (e.g. 10.Qxd5 Qxd5 11.Nf6+ gxf6 12.Bxd5);
/// (b) check-adjusted baseline: if the engine is in check in position k (its move is
///     forced) and bal(k-2) < bal(k), the baseline is position k-2 instead of k — bal(k)
///     was inflated mid-trade because the opponent delayed a recapture with a check
///     (e.g. 9.Nxe5 Qa5+ 10.Nd2 Nxe5, or 27...Rxf3 28.Nf6+ gxf6 29.Rxf3). d2/d4/d6, the
///     composition change and the recoup test are then measured from position k-2.
///
/// (c) persistence after the engine's own reply: the deficit must still be at least the
///     category's d4 threshold in position k+3, i.e. after the engine's next move. Without
///     it, opponent-initiated even trades sampled mid-trade at k+2/k+4/k+6 (12.Nxf6+ Bxf6
///     13.dxe5 Nxe5 14.Nxe5 ...) looked like a persistent deficit. Exception: the engine's
///     move at k+2 is a capture that the opponent recaptures on the same square at k+3
///     (an engine-initiated trade; position k+3 is then mid-trade and d4 decides).
///
/// None of the guards applies when the engine is mating: its own score at k or k+2 is
/// a mate score for it, or it delivers mate by ply k+6 (e.g. 40...Qxf2 41.Rg1+ Qxg1+
/// 42.Kxg1 f2+ 43.Kh2 f1=Q 44.b3 Qg2#: measured from before 40...Qxf2 the queen-for-
/// rook sacrifice would be hidden by guard (b)).
/// Guard (a) applies only to an actual trade: bal(k+5) and bal(k+6) both within
/// +-TRADE_GUARD_MARGIN of the baseline (a combination that wins *more* than the
/// material back is kept as a sacrifice and counts as recouped).
pub const TRADE_GUARD_MARGIN: i32 = 50;
/// Position context of an event from the engine's own score e0 (addition to the spec):
/// `winning` if e0 >= +300, `losing` if e0 <= -300, else `balanced`.
pub const CONTEXT_CP: i32 = 300;

// ---------------------------------------------------------------- classification (pure)

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    Queen,
    Exchange,
    Piece,
    Pawn,
}

impl Category {
    pub const ALL: [Category; 4] = [Category::Queen, Category::Exchange, Category::Piece, Category::Pawn];
    pub fn as_str(&self) -> &'static str {
        match self {
            Category::Queen => "queen",
            Category::Exchange => "exchange",
            Category::Piece => "piece",
            Category::Pawn => "pawn",
        }
    }
}

/// Category of a material deficit. `net[p]` = pieces of type p (P,N,B,R,Q) lost by
/// the engine minus pieces of that type lost by the opponent between position k and
/// position k+4. An engine promotion counts as the promoted piece appearing (a
/// negative loss); an *opponent* promotion is neutral (its pawn is not counted as
/// lost nor the new piece as gained), so allowing a promotion is never a "queen
/// sacrifice". `engine_lost_q` = the engine actually lost a queen. Precedence queen >
/// exchange > piece > pawn; the first category whose composition pattern *and*
/// thresholds hold wins.
pub fn classify(d2: i32, d4: i32, d6: i32, net: &[i32; 5], engine_lost_q: bool) -> Option<Category> {
    let (p, minors, r, q) = (net[0], net[1] + net[2], net[3], net[4]);
    if q >= 1 && engine_lost_q && d2 >= QUEEN_D2 && d4 >= QUEEN_D4 {
        return Some(Category::Queen);
    }
    let exchange_pattern = q == 0 && r == 1 && minors == -1 && p.abs() <= 1;
    if exchange_pattern && d2 >= EXCH_D2 && d4 >= EXCH_D4 {
        return Some(Category::Exchange);
    }
    if !exchange_pattern && (minors >= 1 || r >= 1) && d2 >= PIECE_D2 && d4 >= PIECE_D4 {
        return Some(Category::Piece);
    }
    if q == 0 && r == 0 && minors == 0 && p >= 1 && d2 >= PAWN_D && d4 >= PAWN_D && d6 >= PAWN_D {
        return Some(Category::Pawn);
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// e1 >= e0 - 150: the engine saw it coming.
    Sound,
    /// e0 or e1 is a mate score in the engine's favour.
    Mate,
    /// e0 or e1 missing (book move, no info, game over) — counted as a sacrifice.
    Unknown,
    /// e1 < e0 - 150: the engine's own evaluation collapsed.
    Blunder,
    /// The engine's score had already fallen by more than INTENT_DROP since its previous
    /// move (k-2): the opponent's move k-1 made the loss unavoidable. Counted as a
    /// blunder of the previous move, not as a sacrifice at k.
    Collapsed,
    /// Exactly one legal move at k: no choice, so not a sacrifice (excluded).
    Forced,
    /// e0 is a mate score against the engine: interpositions / spite checks that only
    /// delay mate (excluded).
    Desperation,
}

impl Intent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Intent::Sound => "sound",
            Intent::Mate => "mate",
            Intent::Unknown => "unknown",
            Intent::Blunder => "blunder",
            Intent::Collapsed => "collapsed",
            Intent::Forced => "forced",
            Intent::Desperation => "desperation",
        }
    }
    #[allow(dead_code)] // used by tests
    pub fn is_sacrifice(&self) -> bool {
        matches!(self, Intent::Sound | Intent::Mate | Intent::Unknown)
    }
    fn kind(&self) -> Kind {
        match self {
            Intent::Sound | Intent::Mate | Intent::Unknown => Kind::Sacrifice,
            Intent::Blunder | Intent::Collapsed => Kind::Blunder,
            Intent::Forced => Kind::Forced,
            Intent::Desperation => Kind::Desperation,
        }
    }
}

fn mate_for(s: Option<i32>) -> bool {
    matches!(s, Some(v) if v > 0 && is_mate_score(v))
}

fn mate_against(s: Option<i32>) -> bool {
    matches!(s, Some(v) if v < 0 && is_mate_score(v))
}

/// Full judgement of a candidate at k: `e_prev` = the engine's score at its previous
/// move (k-2), `legal` = number of legal moves in position k.
/// Order: desperation (e0 = being mated) > forced (single legal move; a blunder of the
/// previous move if the score had collapsed) > mate intent > collapsed > `intent`.
pub fn judge(e0: Option<i32>, e1: Option<i32>, e_prev: Option<i32>, legal: usize) -> Intent {
    if mate_against(e0) {
        return Intent::Desperation;
    }
    let collapsed = match (e_prev, e0) {
        (Some(a), Some(b)) => !mate_for(Some(b)) && (a as i64) - (b as i64) > INTENT_DROP as i64,
        _ => false,
    };
    if legal == 1 {
        return if collapsed { Intent::Collapsed } else { Intent::Forced };
    }
    let it = intent(e0, e1);
    if it == Intent::Mate {
        return it;
    }
    if collapsed {
        return Intent::Collapsed;
    }
    it
}

/// Intent filter on the engine's own scores (engine's view, mates ±(100000-n)), the
/// spec rule; `judge` adds the desperation / forced / collapsed cases on top.
pub fn intent(e0: Option<i32>, e1: Option<i32>) -> Intent {
    if mate_for(e0) || mate_for(e1) {
        return Intent::Mate;
    }
    match (e0, e1) {
        (Some(a), Some(b)) => {
            if b >= a - INTENT_DROP {
                Intent::Sound
            } else {
                Intent::Blunder
            }
        }
        _ => Intent::Unknown,
    }
}

/// Map a raw score (cp, mates ±(100000-n)) to the SF scale used in the output.
pub fn sf_norm(raw: i32) -> i32 {
    if is_mate_score(raw) {
        if raw > 0 {
            SF_MATE
        } else {
            -SF_MATE
        }
    } else {
        raw.clamp(-SF_MATE, SF_MATE)
    }
}

// ---------------------------------------------------------------- replay

type Counts = [i32; 5];

fn counts_of(b: &Board, c: Color) -> Counts {
    let mut out = [0; 5];
    for (i, p) in PIECES.iter().enumerate() {
        out[i] = b.colored_pieces(c, *p).len() as i32;
    }
    out
}

fn material(c: &Counts) -> i32 {
    c.iter().zip(VALUES.iter()).map(|(n, v)| n * v).sum()
}

fn cidx(c: Color) -> usize {
    if c == Color::White {
        0
    } else {
        1
    }
}

/// All positions of a game (0..=n) with piece counts.
pub struct Replay {
    pub boards: Vec<Board>,
    counts: Vec<[Counts; 2]>,
}

impl Replay {
    pub fn new(moves: &[String]) -> Result<Replay, String> {
        let mut b = Board::default();
        let mut boards = Vec::with_capacity(moves.len() + 1);
        let mut counts = Vec::with_capacity(moves.len() + 1);
        counts.push([counts_of(&b, Color::White), counts_of(&b, Color::Black)]);
        boards.push(b.clone());
        for (i, m) in moves.iter().enumerate() {
            let mv = san::parse_uci(&b, m).ok_or_else(|| format!("illegal or unparsable move '{}' at ply {}", m, i))?;
            b.play_unchecked(mv);
            counts.push([counts_of(&b, Color::White), counts_of(&b, Color::Black)]);
            boards.push(b.clone());
        }
        Ok(Replay { boards, counts })
    }

    pub fn n(&self) -> usize {
        self.boards.len() - 1
    }

    /// Side that is checkmated in the final position, if any.
    pub fn mated_side(&self) -> Option<Color> {
        let b = &self.boards[self.n()];
        if !san::has_legal_move(b) && !b.checkers().is_empty() {
            Some(b.side_to_move())
        } else {
            None
        }
    }

    /// Value of a terminal position (no legal moves) from the side to move, raw encoding.
    fn terminal_value(&self, i: usize) -> Option<i32> {
        let b = &self.boards[i];
        if san::has_legal_move(b) {
            None
        } else if b.checkers().is_empty() {
            Some(0)
        } else {
            Some(-MATE_SCORE)
        }
    }
}

// ---------------------------------------------------------------- per-game analysis

/// One game seen from the analysed engine's side.
struct Persp<'a> {
    rec: &'a GameRecord,
    rep: &'a Replay,
    color: Color,
    guards: bool,
}

#[derive(Debug, Clone)]
struct Cand {
    /// baseline position (k, or k-2 with the check-adjusted baseline)
    base: usize,
    cat: Category,
    d2: i32,
    d4: i32,
    d6: i32,
    /// deficits after the engine's own replies (guard (c))
    d3: i32,
    d5: i32,
    /// pieces lost between base and k+4; opponent promotions are neutral (see classify)
    lost_e: Counts,
    lost_o: Counts,
    net: Counts,
    /// opponent promotions between base and k+4
    opp_promotions: usize,
    /// the engine is mating (mate score at k or k+2, or it mates by ply k+6): guards
    /// (a) and (c) do not apply
    mating: bool,
}

impl<'a> Persp<'a> {
    fn n(&self) -> usize {
        self.rep.n()
    }

    fn bal(&self, i: usize) -> i32 {
        let c = &self.rep.counts[i.min(self.n())];
        material(&c[cidx(self.color)]) - material(&c[cidx(!self.color)])
    }

    fn counts(&self, i: usize, side: Color) -> Counts {
        self.rep.counts[i.min(self.n())][cidx(side)]
    }

    fn is_engine_ply(&self, i: usize) -> bool {
        (i % 2 == 0) == (self.color == Color::White)
    }

    /// Engine score (engine's view) for the engine move at ply `i`.
    fn score(&self, i: usize) -> Option<i32> {
        if i < self.n() && self.is_engine_ply(i) {
            self.rec.scores[i]
        } else {
            None
        }
    }

    /// 1 / 0.5 / 0 for the engine.
    fn result(&self) -> f64 {
        self.rec.result.score_for(self.color)
    }

    fn result_str(&self) -> &'static str {
        let s = self.result();
        if s > 0.75 {
            "win"
        } else if s < 0.25 {
            "loss"
        } else {
            "draw"
        }
    }

    /// Baseline position for a candidate at k (trade guard (b); not while mating).
    fn baseline(&self, k: usize) -> usize {
        if self.guards
            && k >= 2
            && !self.rep.boards[k].checkers().is_empty()
            && self.bal(k - 2) < self.bal(k)
            && !self.mating(k)
        {
            k - 2
        } else {
            k
        }
    }

    /// Promotions by the opponent in plies [from, to): counts per promoted piece type.
    fn opp_promotions(&self, from: usize, to: usize) -> Counts {
        let mut c = [0; 5];
        for i in from..to.min(self.n()) {
            if self.is_engine_ply(i) {
                continue;
            }
            let m = &self.rec.moves[i];
            if m.len() == 5 {
                let idx = match &m[4..5] {
                    "n" => 1,
                    "b" => 2,
                    "r" => 3,
                    "q" => 4,
                    _ => continue,
                };
                c[idx] += 1;
            }
        }
        c
    }

    /// The engine's move at ply i captures and the opponent's reply at i+1 lands on the
    /// same square (recaptures): position i+1 is in the middle of a trade.
    fn recaptured(&self, i: usize) -> bool {
        if i + 1 >= self.n() {
            return false;
        }
        let b = &self.rep.boards[i];
        let (m, r) = (&self.rec.moves[i], &self.rec.moves[i + 1]);
        match san::parse_uci(b, m) {
            Some(mv) => san::is_capture(b, mv) && r.len() >= 4 && m.len() >= 4 && r[2..4] == m[2..4],
            None => false,
        }
    }

    /// The engine is mating around k: its own score at k or k+2 is a mate score for it,
    /// or the game ends in mate by the engine no later than ply k+6.
    fn mating(&self, k: usize) -> bool {
        mate_for(self.score(k)) || mate_for(self.e1(k)) || matches!(self.engine_mate_ply(), Some(m) if m >= k && m <= k + 6)
    }

    fn candidate(&self, k: usize) -> Option<Cand> {
        let base = self.baseline(k);
        let b0 = self.bal(base);
        let d2 = b0 - self.bal(k + 2);
        let d4 = b0 - self.bal(k + 4);
        let d6 = b0 - self.bal(k + 6);
        let d3 = b0 - self.bal(k + 3);
        let d5 = b0 - self.bal(k + 5);
        if d2 < PAWN_D.min(EXCH_D2) {
            return None; // cheap reject: every category needs d2 >= 100
        }
        let mating = self.mating(k);
        if self.guards
            && !mating
            && (self.bal(k + 5) - b0).abs() <= TRADE_GUARD_MARGIN
            && (self.bal(k + 6) - b0).abs() <= TRADE_GUARD_MARGIN
        {
            return None; // trade guard (a): the engine completed a delayed recapture at k+4
        }
        let me = self.color;
        let (e0, e4) = (self.counts(base, me), self.counts(k + 4, me));
        let (o0, o4) = (self.counts(base, !me), self.counts(k + 4, !me));
        let promo = self.opp_promotions(base, k + 4);
        let opp_promotions = promo.iter().sum::<i32>() as usize;
        let mut lost_e = [0; 5];
        let mut lost_o = [0; 5];
        let mut net = [0; 5];
        for p in 0..5 {
            lost_e[p] = e0[p] - e4[p];
            // neutralise opponent promotions: the promoted pawn was not lost, the new
            // piece was not gained
            lost_o[p] = o0[p] - o4[p] + promo[p];
        }
        lost_o[0] -= opp_promotions as i32;
        for p in 0..5 {
            net[p] = lost_e[p] - lost_o[p];
        }
        let cat = classify(d2, d4, d6, &net, lost_e[4] >= 1)?;
        if self.guards && !mating {
            // guard (c): the deficit must survive the engine's own next move(s)
            let need = match cat {
                Category::Queen => QUEEN_D4,
                Category::Exchange => EXCH_D4,
                Category::Piece => PIECE_D4,
                Category::Pawn => PAWN_D,
            };
            if d3 < need && !self.recaptured(k + 2) {
                return None;
            }
        }
        Some(Cand { base, cat, d2, d4, d6, d3, d5, lost_e, lost_o, net, opp_promotions, mating })
    }

    /// Engine's view one engine-move after k (ply k+2). If the game ended before
    /// that because the engine was mated, that is a lost position (-mate).
    fn e1(&self, k: usize) -> Option<i32> {
        if k + 2 < self.n() {
            return self.score(k + 2);
        }
        if self.rep.mated_side() == Some(self.color) {
            return Some(-MATE_SCORE);
        }
        None
    }

    /// Ply of the engine's mating move if the game ended in mate by the engine.
    fn engine_mate_ply(&self) -> Option<usize> {
        if self.n() > 0 && self.rep.mated_side() == Some(!self.color) {
            Some(self.n() - 1)
        } else {
            None
        }
    }

    /// Engine score at (or shortly after) engine-to-move position j; terminal/ended
    /// games use the result (win = +mate, draw = 0, loss = -mate).
    fn engine_score_at(&self, j: usize) -> Option<i32> {
        if j >= self.n() {
            let s = self.result();
            return Some(if s > 0.75 {
                MATE_SCORE
            } else if s < 0.25 {
                -MATE_SCORE
            } else {
                0
            });
        }
        let mut jj = j;
        while jj < self.n() && jj <= j + 4 {
            if let Some(s) = self.score(jj) {
                return Some(s);
            }
            jj += 2;
        }
        None
    }

    fn fullmove(&self, i: usize) -> usize {
        i / 2 + 1
    }

    fn san_line(&self, from: usize, to: usize) -> String {
        let mut out = String::new();
        let to = to.min(self.n());
        for i in from..to {
            if !out.is_empty() {
                out.push(' ');
            }
            if i % 2 == 0 {
                out.push_str(&format!("{}.", self.fullmove(i)));
            } else if i == from {
                out.push_str(&format!("{}...", self.fullmove(i)));
            }
            out.push_str(&self.rec.san[i]);
        }
        out
    }
}

/// `winning` / `balanced` / `losing` from the engine's own score when it moved.
pub fn context(e0: Option<i32>) -> &'static str {
    match e0 {
        None => "unknown",
        Some(v) if v >= CONTEXT_CP => "winning",
        Some(v) if v <= -CONTEXT_CP => "losing",
        Some(_) => "balanced",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Sacrifice,
    Blunder,
    /// single legal move (excluded from sacrifices and blunders)
    Forced,
    /// moves while being mated (excluded from sacrifices and blunders)
    Desperation,
}

impl Kind {
    fn as_str(&self) -> &'static str {
        match self {
            Kind::Sacrifice => "sacrifice",
            Kind::Blunder => "blunder",
            Kind::Forced => "forced",
            Kind::Desperation => "desperation",
        }
    }
    fn idx(&self) -> usize {
        match self {
            Kind::Sacrifice => 0,
            Kind::Blunder => 1,
            Kind::Forced => 2,
            Kind::Desperation => 3,
        }
    }
}

#[derive(Debug, Clone)]
struct Followup {
    ply: usize,
    cat: Category,
    intent: Intent,
}

/// A detected sacrifice or blunder (engine's view throughout).
#[derive(Debug, Clone)]
struct Event {
    kind: Kind,
    game: usize, // index into the global game list
    persp: usize, // index into the perspective list
    k: usize,
    /// position whose Stockfish before/after evals judge the move: k, or k-2 for a
    /// `collapsed` blunder (the loss was caused by the previous move)
    eval_ply: usize,
    cand: Cand,
    e0: Option<i32>,
    e1: Option<i32>,
    /// engine's score at its previous move (k-2)
    e_prev: Option<i32>,
    legal_moves: usize,
    intent: Intent,
    book: bool,
    truncated: bool,
    chain: Vec<Followup>,
    chain_len: usize,
    // outcome
    mate_ply: Option<usize>,
    recouped_ply: Option<usize>,
    recoup_score: Option<i32>,
    recoup_success: bool,
    result: f64,
    // Stockfish (engine view, ±3000)
    sf_before: Option<i32>,
    sf_after: Option<i32>,
    sf_later: Option<i32>,
    sf_best_later: Option<(i32, usize)>,
    unsound: Option<bool>,
    practical: Option<bool>,
    successful: bool,
    success_reason: Option<&'static str>,
}

/// Detect events in one perspective (dedup / chain logic).
///
/// Every event opens a window of its own kind up to its ply + 4. A candidate inside the
/// window of an open *sacrifice* is a follow-up of that chain: a further sacrifice
/// (passes the intent filter) extends the window by 4 plies and counts in `chain_len`;
/// a forced / desperation move is only listed in the chain; a blunder neither extends
/// nor is absorbed - it becomes (or joins) a blunder event and counts as a blunder.
/// Inside a blunder / forced / desperation window, candidates of the same kind are
/// absorbed (extending that window); anything else starts a new event.
fn detect(p: &Persp, game: usize, persp: usize) -> Vec<Event> {
    let mut out: Vec<Event> = Vec::new();
    let n = p.n();
    let start = p.rec.opening_plies;
    // per kind: (event index, last ply of the window)
    let mut win: [Option<(usize, usize)>; 4] = [None; 4];
    let mut k = start;
    while k < n {
        if !p.is_engine_ply(k) {
            k += 1;
            continue;
        }
        let Some(c) = p.candidate(k) else {
            k += 2;
            continue;
        };
        let e0 = p.score(k);
        let e1 = p.e1(k);
        let e_prev = if k >= 2 { p.score(k - 2) } else { None };
        let legal = san::legal_moves(&p.rep.boards[k]).len();
        let it = judge(e0, e1, e_prev, legal);
        let kind = it.kind();
        if let Some((si, until)) = win[Kind::Sacrifice.idx()] {
            if k <= until {
                match kind {
                    Kind::Sacrifice => {
                        out[si].chain_len += 1;
                        out[si].chain.push(Followup { ply: k, cat: c.cat, intent: it });
                        win[Kind::Sacrifice.idx()] = Some((si, k + 4));
                        k += 2;
                        continue;
                    }
                    Kind::Forced | Kind::Desperation => {
                        out[si].chain.push(Followup { ply: k, cat: c.cat, intent: it });
                        k += 2;
                        continue;
                    }
                    Kind::Blunder => {
                        // listed in the chain for reference, counted as a blunder below
                        out[si].chain.push(Followup { ply: k, cat: c.cat, intent: it });
                    }
                }
            }
        }
        if kind != Kind::Sacrifice {
            if let Some((bi, until)) = win[kind.idx()] {
                if k <= until {
                    out[bi].chain.push(Followup { ply: k, cat: c.cat, intent: it });
                    win[kind.idx()] = Some((bi, k + 4));
                    k += 2;
                    continue;
                }
            }
        }
        let mut ev = Event {
            kind,
            game,
            persp,
            k,
            eval_ply: if it == Intent::Collapsed && k >= 2 { k - 2 } else { k },
            truncated: k + 6 > n,
            cand: c,
            e0,
            e1,
            e_prev,
            legal_moves: legal,
            intent: it,
            book: p.rec.book.get(k).copied().unwrap_or(false),
            chain: Vec::new(),
            chain_len: 1,
            mate_ply: None,
            recouped_ply: None,
            recoup_score: None,
            recoup_success: false,
            result: p.result(),
            sf_before: None,
            sf_after: None,
            sf_later: None,
            sf_best_later: None,
            unsound: None,
            practical: None,
            successful: false,
            success_reason: None,
        };
        // outcome (material / engine score part)
        if let Some(m) = p.engine_mate_ply() {
            if m >= k && m - k <= MATE_WINDOW {
                ev.mate_ply = Some(m);
            }
        }
        let b0 = p.bal(ev.cand.base);
        let mut j = k + 6;
        let last = (k + RECOUP_WINDOW).min(n);
        let mut cands: Vec<usize> = Vec::new();
        while j <= last {
            cands.push(j);
            j += 2;
        }
        if n <= k + RECOUP_WINDOW && n > k + 4 && !cands.contains(&n) {
            cands.push(n);
        }
        for j in cands {
            if p.bal(j) >= b0 - RECOUP_MARGIN {
                ev.recouped_ply = Some(j);
                ev.recoup_score = p.engine_score_at(j);
                let base = e0.unwrap_or(0);
                ev.recoup_success = matches!(ev.recoup_score, Some(s) if s >= base - SUCCESS_SCORE_MARGIN);
                break;
            }
        }
        out.push(ev);
        win[kind.idx()] = Some((out.len() - 1, k + 4));
        k += 2;
    }
    out
}

// ---------------------------------------------------------------- statistics helpers

const Z95: f64 = 1.959964;

/// Wilson score interval for k successes out of n.
pub fn wilson(k: usize, n: usize) -> Option<(f64, f64)> {
    wilson_f(k as f64, n)
}

/// Wilson interval with a fractional success count (score points).
pub fn wilson_f(kf: f64, n: usize) -> Option<(f64, f64)> {
    if n == 0 {
        return None;
    }
    let nf = n as f64;
    let p = kf / nf;
    let z2 = Z95 * Z95;
    let denom = 1.0 + z2 / nf;
    let centre = (p + z2 / (2.0 * nf)) / denom;
    let half = Z95 * (p * (1.0 - p) / nf + z2 / (4.0 * nf * nf)).sqrt() / denom;
    Some(((centre - half).max(0.0), (centre + half).min(1.0)))
}

fn r4(x: f64) -> f64 {
    (x * 10000.0).round() / 10000.0
}

fn prop(k: usize, n: usize) -> Json {
    let mut j = Json::obj().with("count", k).with("of", n);
    match wilson(k, n) {
        Some((lo, hi)) => {
            j.set("rate", r4(k as f64 / n as f64));
            j.set("ci95", vec![r4(lo), r4(hi)]);
        }
        None => {
            j.set("rate", Json::Null);
            j.set("ci95", Json::Null);
        }
    }
    j
}

/// Mean, sample SD and normal-approximation 95% CI of the mean, clipped to [lo, hi]
/// (the natural range of the quantity, e.g. counts >= 0).
fn mean_ci_in(v: &[f64], lo: f64, hi: f64) -> Json {
    let n = v.len();
    if n == 0 {
        return Json::obj().with("n", 0).with("mean", Json::Null).with("sd", Json::Null).with("ci95", Json::Null);
    }
    let mean = v.iter().sum::<f64>() / n as f64;
    let sd = if n > 1 { (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64).sqrt() } else { 0.0 };
    let half = if n > 1 { Z95 * sd / (n as f64).sqrt() } else { f64::NAN };
    let ci = if half.is_finite() {
        Json::from(vec![r4((mean - half).max(lo)), r4((mean + half).min(hi))])
    } else {
        Json::Null
    };
    Json::obj().with("n", n).with("mean", r4(mean)).with("sd", r4(sd)).with("ci95", ci)
}

fn mean_ci(v: &[f64]) -> Json {
    mean_ci_in(v, f64::NEG_INFINITY, f64::INFINITY)
}

/// Means of non-negative quantities (per-game counts, move numbers).
fn mean_ci_pos(v: &[f64]) -> Json {
    mean_ci_in(v, 0.0, f64::INFINITY)
}

/// W/D/L with the mean score; its 95% CI is the Wilson interval on score points
/// (draws = half points), which stays inside [0, 1] and does not collapse at 0% / 100%
/// (conservative when there are draws).
fn wdl_json(scores: &[f64]) -> Json {
    let w = scores.iter().filter(|&&s| s > 0.75).count();
    let l = scores.iter().filter(|&&s| s < 0.25).count();
    let d = scores.len() - w - l;
    let mut sc = mean_ci_in(scores, 0.0, 1.0);
    let points = w as f64 + 0.5 * d as f64;
    sc.set("points", points);
    if let Some((lo, hi)) = wilson_f(points, scores.len()) {
        sc.set("ci95", vec![r4(lo), r4(hi)]);
    }
    Json::obj().with("games", scores.len()).with("wins", w).with("draws", d).with("losses", l).with("score", sc)
}

// ---------------------------------------------------------------- aggregation

struct GameStat {
    engine: String,
    opponent: String,
    score: f64,
    sacrifices: usize,
    /// sacrifices played in a `balanced` context (|e0| < 300)
    balanced_sacrifices: usize,
    blunders: usize,
    forced: usize,
    desperation: usize,
    engine_moves: usize,
}

fn category_block(evs: &[&Event], with_sf: bool) -> Json {
    let n = evs.len();
    let count = |f: &dyn Fn(&Event) -> bool| evs.iter().filter(|e| f(e)).count();
    let scores: Vec<f64> = evs.iter().map(|e| e.result).collect();
    let mut j = Json::obj()
        .with("count", n)
        .with("successful_attack", prop(count(&|e| e.successful), n))
        .with("mate", prop(count(&|e| e.mate_ply.is_some()), n))
        .with("recouped", prop(count(&|e| e.recouped_ply.is_some()), n))
        .with("book", count(&|e| e.book))
        .with("intent_unknown", count(&|e| e.intent == Intent::Unknown))
        .with("game_result", wdl_json(&scores))
        .with("avg_d4", mean_ci_pos(&evs.iter().map(|e| e.cand.d4 as f64).collect::<Vec<_>>()))
        .with("avg_move_number", mean_ci_pos(&evs.iter().map(|e| (e.k / 2 + 1) as f64).collect::<Vec<_>>()));
    if with_sf {
        let evald: Vec<&&Event> = evs.iter().filter(|e| e.unsound.is_some()).collect();
        let unsound = evald.iter().filter(|e| e.unsound == Some(true)).count();
        let practical = evald.iter().filter(|e| e.practical == Some(true)).count();
        j.set("sf_evaluated", evald.len());
        j.set("unsound", prop(unsound, evald.len()));
        j.set("practical_chance", prop(practical, unsound));
        let drops: Vec<f64> =
            evald.iter().filter_map(|e| Some((e.sf_before? - e.sf_after?) as f64)).collect::<Vec<_>>();
        j.set("avg_sf_drop", mean_ci(&drops));
        j.set("avg_sf_after", mean_ci_in(&evald.iter().filter_map(|e| e.sf_after.map(|x| x as f64)).collect::<Vec<_>>(), -SF_MATE as f64, SF_MATE as f64));
    }
    j
}

fn group_block(games: &[&GameStat], sacs: &[&Event], blunders: &[&Event], with_sf: bool) -> Json {
    let ng = games.len();
    let scores: Vec<f64> = games.iter().map(|g| g.score).collect();
    let per_game_sacs: Vec<f64> = games.iter().map(|g| g.sacrifices as f64).collect();
    let per_game_bl: Vec<f64> = games.iter().map(|g| g.blunders as f64).collect();
    let per_game_bal: Vec<f64> = games.iter().map(|g| g.balanced_sacrifices as f64).collect();
    let with: Vec<f64> = games.iter().filter(|g| g.sacrifices > 0).map(|g| g.score).collect();
    let without: Vec<f64> = games.iter().filter(|g| g.sacrifices == 0).map(|g| g.score).collect();
    let moves: usize = games.iter().map(|g| g.engine_moves).sum();
    let mut hist: BTreeMap<usize, usize> = BTreeMap::new();
    for g in games {
        *hist.entry(g.sacrifices).or_insert(0) += 1;
    }
    let mut hist_j = Json::obj();
    for (k, v) in &hist {
        hist_j.set(&k.to_string(), *v);
    }
    let mut cats = Json::obj();
    let mut per_cat = Json::obj();
    for c in Category::ALL {
        let evs: Vec<&Event> = sacs.iter().copied().filter(|e| e.cand.cat == c).collect();
        cats.set(c.as_str(), evs.len());
        per_cat.set(c.as_str(), category_block(&evs, with_sf));
    }
    let mut bl_cats = Json::obj();
    for c in Category::ALL {
        bl_cats.set(c.as_str(), blunders.iter().filter(|e| e.cand.cat == c).count());
    }
    let chained: Vec<&&Event> = sacs.iter().filter(|e| e.chain_len > 1).collect();
    let sf_bl: Vec<&&Event> = blunders.iter().filter(|e| e.sf_before.is_some() && e.sf_after.is_some()).collect();
    let bl_confirmed = sf_bl.iter().filter(|e| e.sf_after.unwrap() < e.sf_before.unwrap() - UNSOUND_DROP).count();
    let mut j = Json::obj()
        .with("games", ng)
        .with("result", wdl_json(&scores))
        .with("engine_moves_analysed", moves)
        .with("games_with_sacrifice", prop(with.len(), ng))
        .with("sacrifices", sacs.len())
        .with("sacrifices_per_game", mean_ci_pos(&per_game_sacs))
        .with("sacrifices_per_100_moves", if moves > 0 { Json::from(r4(100.0 * sacs.len() as f64 / moves as f64)) } else { Json::Null })
        .with("sacrifices_per_game_histogram", hist_j)
        .with("categories", cats)
        .with("book_sacrifices", sacs.iter().filter(|e| e.book).count())
        .with("intent_unknown", sacs.iter().filter(|e| e.intent == Intent::Unknown).count())
        .with("intent_mate", sacs.iter().filter(|e| e.intent == Intent::Mate).count())
        .with("chained_sacrifices", chained.len())
        .with("avg_chain_len", mean_ci_pos(&sacs.iter().map(|e| e.chain_len as f64).collect::<Vec<_>>()))
        .with("sacrifices_by_context", {
            let mut c = Json::obj();
            for name in ["balanced", "winning", "losing", "unknown"] {
                c.set(name, sacs.iter().filter(|e| context(e.e0) == name).count());
            }
            c
        })
        .with("balanced_sacrifices_per_game", mean_ci_pos(&per_game_bal))
        .with("games_with_balanced_sacrifice", prop(games.iter().filter(|g| g.balanced_sacrifices > 0).count(), ng))
        .with("blunders", blunders.len())
        .with("blunders_per_game", mean_ci_pos(&per_game_bl))
        .with("blunder_categories", bl_cats)
        .with("blunders_collapsed", blunders.iter().filter(|e| e.intent == Intent::Collapsed).count())
        .with("forced_losses", games.iter().map(|g| g.forced).sum::<usize>())
        .with("desperation_moves", games.iter().map(|g| g.desperation).sum::<usize>())
        .with("mates_after_sacrifice", prop(sacs.iter().filter(|e| e.mate_ply.is_some()).count(), sacs.len()))
        .with("successful_attacks", prop(sacs.iter().filter(|e| e.successful).count(), sacs.len()))
        .with("recouped", prop(sacs.iter().filter(|e| e.recouped_ply.is_some()).count(), sacs.len()))
        .with("score_games_with_sacrifice", wdl_json(&with))
        .with("score_games_without_sacrifice", wdl_json(&without))
        .with("sacrifice_game_result", wdl_json(&sacs.iter().map(|e| e.result).collect::<Vec<_>>()))
        .with("avg_move_number", mean_ci_pos(&sacs.iter().map(|e| (e.k / 2 + 1) as f64).collect::<Vec<_>>()));
    if with_sf {
        let evald: Vec<&&Event> = sacs.iter().filter(|e| e.unsound.is_some()).collect();
        let unsound = evald.iter().filter(|e| e.unsound == Some(true)).count();
        let practical = evald.iter().filter(|e| e.practical == Some(true)).count();
        j.set("sf_evaluated", evald.len());
        j.set("unsound", prop(unsound, evald.len()));
        j.set("practical_chances", prop(practical, unsound));
        j.set("sound_sacrifices", prop(evald.len() - unsound, evald.len()));
        j.set(
            "avg_sf_before",
            mean_ci_in(&evald.iter().filter_map(|e| e.sf_before.map(|x| x as f64)).collect::<Vec<_>>(), -SF_MATE as f64, SF_MATE as f64),
        );
        j.set("avg_sf_after", mean_ci_in(&evald.iter().filter_map(|e| e.sf_after.map(|x| x as f64)).collect::<Vec<_>>(), -SF_MATE as f64, SF_MATE as f64));
        j.set("blunders_confirmed_by_sf", prop(bl_confirmed, sf_bl.len()));
    }
    j.set("per_category", per_cat);
    j
}

// ---------------------------------------------------------------- command

struct Opts {
    inputs: Vec<PathBuf>,
    engines: Vec<String>,
    sf: Option<SfSettings>,
    out: Option<String>,
    quiet: bool,
    guards: bool,
}

const USAGE: &str = "usage: aggro_arena analyze <games.jsonl>... --engine <name>[,<name>...] [--engine <name>]...
                    [--sf <stockfish-path>] [--sf-nodes N (400000)] [--threads N (12)] [--sf-hash MB (64)]
                    [--out <prefix>] [--quiet] [--no-trade-guards]
  writes <prefix>.json (summary) and <prefix>.events.jsonl (one line per sacrifice / blunder);
  default prefix: first input without .jsonl + '.sac'";

fn parse_opts(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts { inputs: Vec::new(), engines: Vec::new(), sf: None, out: None, quiet: false, guards: true };
    let mut sf_path: Option<String> = None;
    let mut nodes: u64 = 400_000;
    let mut threads: usize = 12;
    let mut hash: u32 = 64;
    let mut i = 0;
    let val = |i: usize| -> Result<&String, String> { args.get(i + 1).ok_or_else(|| format!("{} needs a value", args[i])) };
    while i < args.len() {
        match args[i].as_str() {
            "--engine" | "-e" => {
                for n in val(i)?.split(',') {
                    let n = n.trim();
                    if !n.is_empty() && !o.engines.iter().any(|x| x == n) {
                        o.engines.push(n.to_string());
                    }
                }
                i += 2;
            }
            "--sf" => {
                sf_path = Some(val(i)?.clone());
                i += 2;
            }
            "--sf-nodes" => {
                nodes = val(i)?.parse().map_err(|_| "bad --sf-nodes")?;
                if nodes == 0 {
                    return Err("--sf-nodes must be > 0".into());
                }
                i += 2;
            }
            "--threads" | "--sf-threads" => {
                threads = val(i)?.parse().map_err(|_| "bad --threads")?;
                if threads == 0 {
                    return Err("--threads must be > 0".into());
                }
                i += 2;
            }
            "--sf-hash" => {
                hash = val(i)?.parse().map_err(|_| "bad --sf-hash")?;
                i += 2;
            }
            "--out" | "-o" => {
                o.out = Some(val(i)?.clone());
                i += 2;
            }
            "--quiet" | "-q" => {
                o.quiet = true;
                i += 1;
            }
            "--no-trade-guards" => {
                o.guards = false;
                i += 1;
            }
            a if a.starts_with('-') => return Err(format!("unknown option '{}'", a)),
            a => {
                o.inputs.push(PathBuf::from(a));
                i += 1;
            }
        }
    }
    if o.inputs.is_empty() {
        return Err("no input files".into());
    }
    if o.engines.is_empty() {
        return Err("--engine is required".into());
    }
    if let Some(path) = sf_path {
        o.sf = Some(SfSettings { path, nodes, threads, hash_mb: hash });
    }
    Ok(o)
}

pub fn cmd(args: &[String]) -> i32 {
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        errln!("{}", USAGE);
        return if args.is_empty() { 2 } else { 0 };
    }
    let opts = match parse_opts(args) {
        Ok(o) => o,
        Err(e) => {
            errln!("analyze: {}\n{}", e, USAGE);
            return 2;
        }
    };
    match run(opts) {
        Ok(code) => code,
        Err(e) => {
            errln!("analyze: error: {}", e);
            1
        }
    }
}

/// Key of an SF evaluation: (global game index, position index).
fn pos_key(game: usize, ply: usize) -> u64 {
    ((game as u64) << 20) | ply as u64
}

fn position_cmd(rec: &GameRecord, ply: usize) -> String {
    if ply == 0 {
        "startpos".to_string()
    } else {
        format!("startpos moves {}", rec.moves[..ply].join(" "))
    }
}

struct LoadedGame {
    file: usize,
    rec: GameRecord,
    rep: Replay,
}

struct PerspInfo {
    game: usize,
    color: Color,
    engine: String,
    opponent: String,
}

fn run(opts: Opts) -> Result<i32, String> {
    sys::install_signal_handlers();
    let started = sys::now_iso_utc();
    let prefix = match &opts.out {
        Some(p) => p.clone(),
        None => {
            let s = opts.inputs[0].to_string_lossy().to_string();
            format!("{}.sac", s.strip_suffix(".jsonl").unwrap_or(&s))
        }
    };

    // ---- load
    let mut games: Vec<LoadedGame> = Vec::new();
    let mut in_files = 0usize;
    let mut invalid = 0usize;
    for (fi, path) in opts.inputs.iter().enumerate() {
        let recs = game::read_jsonl(path)?;
        in_files += recs.len();
        for rec in recs {
            match Replay::new(&rec.moves) {
                Ok(rep) => games.push(LoadedGame { file: fi, rec, rep }),
                Err(e) => {
                    invalid += 1;
                    errln!("warning: {} game {}: {} (skipped)", path.display(), rec.id, e);
                }
            }
        }
    }

    // ---- perspectives: exact engine-name match, either colour
    let mut persps: Vec<PerspInfo> = Vec::new();
    let mut no_engine = 0usize;
    let mut both_sides = 0usize;
    for (gi, g) in games.iter().enumerate() {
        let w = opts.engines.iter().any(|e| *e == g.rec.white);
        let b = opts.engines.iter().any(|e| *e == g.rec.black);
        if w && b {
            both_sides += 1;
        }
        if w {
            persps.push(PerspInfo { game: gi, color: Color::White, engine: g.rec.white.clone(), opponent: g.rec.black.clone() });
        }
        if b {
            persps.push(PerspInfo { game: gi, color: Color::Black, engine: g.rec.black.clone(), opponent: g.rec.white.clone() });
        }
        if !w && !b {
            no_engine += 1;
        }
    }
    if both_sides > 0 {
        errln!(
            "warning: {} game(s) have analysed engines on both sides; each side is analysed separately (counted twice)",
            both_sides
        );
    }
    if persps.is_empty() {
        return Err(format!("no game played by engine(s) {:?} in the input ({} games read)", opts.engines, in_files));
    }

    // ---- detection
    let mut events: Vec<Event> = Vec::new();
    for (pi, pinfo) in persps.iter().enumerate() {
        let g = &games[pinfo.game];
        let p = Persp { rec: &g.rec, rep: &g.rep, color: pinfo.color, guards: opts.guards };
        events.extend(detect(&p, pinfo.game, pi));
    }

    // ---- Stockfish
    let mut sf_info = Json::Null;
    if let Some(sf) = &opts.sf {
        let id = sf.check().map_err(|e| format!("Stockfish: {}", e))?;
        let mut failed = 0usize;
        let mut restarts = 0usize;
        let mut evaluated = 0usize;
        // phase 1: before/after for every event, k+12 for sacrifices
        let mut jobs: Vec<EvalJob> = Vec::new();
        let mut local: std::collections::HashMap<u64, i32> = std::collections::HashMap::new();
        let want = |game: usize, ply: usize, jobs: &mut Vec<EvalJob>, local: &mut std::collections::HashMap<u64, i32>| {
            let g = &games[game];
            if let Some(v) = g.rep.terminal_value(ply) {
                local.insert(pos_key(game, ply), v);
            } else {
                jobs.push(EvalJob { key: pos_key(game, ply), position: position_cmd(&g.rec, ply) });
            }
        };
        for e in &events {
            if !matches!(e.kind, Kind::Sacrifice | Kind::Blunder) {
                continue; // forced / desperation moves are not judged
            }
            let n = games[e.game].rep.n();
            want(e.game, e.eval_ply, &mut jobs, &mut local);
            want(e.game, e.eval_ply + 1, &mut jobs, &mut local);
            if e.kind == Kind::Sacrifice && e.k + SF_LATER_OFFSET <= n {
                want(e.game, e.k + SF_LATER_OFFSET, &mut jobs, &mut local);
            }
        }
        let b1 = sfeval::evaluate(sf, &jobs, "sf phase 1");
        if b1.aborted {
            errln!("interrupted");
            return Ok(130);
        }
        failed += b1.failed.len();
        restarts += b1.restarts;
        evaluated += b1.scores.len();
        local.extend(b1.scores);
        let view = |games: &Vec<LoadedGame>, local: &std::collections::HashMap<u64, i32>, e: &Event, ply: usize| -> Option<i32> {
            let raw = *local.get(&pos_key(e.game, ply))?;
            let color = persps[e.persp].color;
            let stm = games[e.game].rep.boards[ply].side_to_move();
            Some(sf_norm(if stm == color { raw } else { -raw }))
        };
        for e in events.iter_mut() {
            if !matches!(e.kind, Kind::Sacrifice | Kind::Blunder) {
                continue;
            }
            e.sf_before = view(&games, &local, e, e.eval_ply);
            e.sf_after = view(&games, &local, e, e.eval_ply + 1);
            if e.kind == Kind::Sacrifice {
                e.sf_later = view(&games, &local, e, e.k + SF_LATER_OFFSET);
                if let (Some(b), Some(a)) = (e.sf_before, e.sf_after) {
                    e.unsound = Some(a < b - UNSOUND_DROP && a < UNSOUND_ABS);
                }
            }
        }
        // phase 2: later engine-to-move positions for unsound sacrifices in lost games
        let mut jobs2: Vec<EvalJob> = Vec::new();
        for e in &events {
            if e.unsound == Some(true) && e.result < 0.25 {
                let n = games[e.game].rep.n();
                let mut j = e.k + 2;
                while j <= e.k + PRACTICAL_WINDOW && j < n {
                    if !local.contains_key(&pos_key(e.game, j)) {
                        want(e.game, j, &mut jobs2, &mut local);
                    }
                    j += 2;
                }
            }
        }
        if !jobs2.is_empty() {
            let b2 = sfeval::evaluate(sf, &jobs2, "sf phase 2");
            if b2.aborted {
                errln!("interrupted");
                return Ok(130);
            }
            failed += b2.failed.len();
            restarts += b2.restarts;
            evaluated += b2.scores.len();
            local.extend(b2.scores);
        }
        for e in events.iter_mut() {
            if e.unsound != Some(true) {
                if e.unsound == Some(false) {
                    e.practical = Some(false);
                }
                continue;
            }
            if e.result >= 0.25 {
                e.practical = Some(true);
                continue;
            }
            let n = games[e.game].rep.n();
            let mut best: Option<(i32, usize)> = None;
            let mut j = e.k + 2;
            while j <= e.k + PRACTICAL_WINDOW && j < n {
                if let Some(v) = view(&games, &local, e, j) {
                    if best.map(|(b, _)| v > b).unwrap_or(true) {
                        best = Some((v, j));
                    }
                }
                j += 2;
            }
            e.sf_best_later = best;
            e.practical = Some(matches!(best, Some((v, _)) if v >= PRACTICAL_SWING));
        }
        sf_info = Json::obj()
            .with("path", sf.path.as_str())
            .with("id", id)
            .with("nodes", sf.nodes)
            .with("threads_per_process", 1)
            .with("processes", sf.threads)
            .with("hash_mb", sf.hash_mb as u64)
            .with("positions_evaluated", evaluated)
            .with("positions_failed", failed)
            .with("restarts", restarts);
    }

    // ---- success
    for e in events.iter_mut() {
        if e.kind != Kind::Sacrifice {
            continue;
        }
        if e.mate_ply.is_some() {
            e.successful = true;
            e.success_reason = Some("mate");
        } else if e.recoup_success {
            e.successful = true;
            e.success_reason = Some("recouped");
        } else if matches!(e.sf_later, Some(v) if v >= SF_SUCCESS) {
            e.successful = true;
            e.success_reason = Some("sf");
        }
    }

    // ---- events file
    let ev_path = format!("{}.events.jsonl", prefix);
    if let Some(dir) = Path::new(&ev_path).parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
        }
    }
    {
        let mut f = std::io::BufWriter::new(std::fs::File::create(&ev_path).map_err(|e| format!("{}: {}", ev_path, e))?);
        for e in &events {
            let pi = &persps[e.persp];
            let g = &games[e.game];
            let p = Persp { rec: &g.rec, rep: &g.rep, color: pi.color, guards: opts.guards };
            writeln!(f, "{}", event_json(e, &p, pi, &opts.inputs[g.file]).to_compact()).map_err(|e| e.to_string())?;
        }
        f.flush().map_err(|e| e.to_string())?;
    }

    // ---- summary
    let stats: Vec<GameStat> = persps
        .iter()
        .enumerate()
        .map(|(pi, p)| {
            let g = &games[p.game];
            let n = g.rep.n();
            let moves = (g.rec.opening_plies..n).filter(|i| (i % 2 == 0) == (p.color == Color::White)).count();
            GameStat {
                engine: p.engine.clone(),
                opponent: p.opponent.clone(),
                score: g.rec.result.score_for(p.color),
                sacrifices: events.iter().filter(|e| e.persp == pi && e.kind == Kind::Sacrifice).count(),
                balanced_sacrifices: events
                    .iter()
                    .filter(|e| e.persp == pi && e.kind == Kind::Sacrifice && context(e.e0) == "balanced")
                    .count(),
                blunders: events.iter().filter(|e| e.persp == pi && e.kind == Kind::Blunder).count(),
                forced: events.iter().filter(|e| e.persp == pi && e.kind == Kind::Forced).count(),
                desperation: events.iter().filter(|e| e.persp == pi && e.kind == Kind::Desperation).count(),
                engine_moves: moves,
            }
        })
        .collect();
    let with_sf = opts.sf.is_some();
    let block = |filter: &dyn Fn(&PerspInfo) -> bool| -> Json {
        let gs: Vec<&GameStat> = stats.iter().enumerate().filter(|(i, _)| filter(&persps[*i])).map(|(_, s)| s).collect();
        let sacs: Vec<&Event> = events.iter().filter(|e| e.kind == Kind::Sacrifice && filter(&persps[e.persp])).collect();
        let bls: Vec<&Event> = events.iter().filter(|e| e.kind == Kind::Blunder && filter(&persps[e.persp])).collect();
        group_block(&gs, &sacs, &bls, with_sf)
    };
    let overall = block(&|_| true);
    let mut opponents: Vec<String> = stats.iter().map(|s| s.opponent.clone()).collect();
    opponents.sort();
    opponents.dedup();
    let mut per_opp = Json::obj();
    for o in &opponents {
        per_opp.set(o, block(&|p: &PerspInfo| p.opponent == *o));
    }
    let mut engines_seen: Vec<String> = stats.iter().map(|s| s.engine.clone()).collect();
    engines_seen.sort();
    engines_seen.dedup();
    let mut summary = Json::obj()
        .with("tool", "aggro_arena analyze")
        .with("format_version", 1)
        .with("created", started)
        .with("inputs", opts.inputs.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>())
        .with("engine_names", opts.engines.clone())
        .with("events_file", ev_path.as_str())
        .with("games_in_files", in_files)
        .with("games_invalid", invalid)
        .with("games_without_engine", no_engine)
        .with("games_with_engine_on_both_sides", both_sides)
        .with("games_analysed", stats.len())
        .with("stockfish", sf_info)
        .with("params", params_json().with("trade_guards", opts.guards))
        .with("overall", overall)
        .with("per_opponent", per_opp);
    if engines_seen.len() > 1 {
        let mut per_eng = Json::obj();
        for en in &engines_seen {
            per_eng.set(en, block(&|p: &PerspInfo| p.engine == *en));
        }
        summary.set("per_engine", per_eng);
    }
    let sum_path = format!("{}.json", prefix);
    std::fs::write(&sum_path, summary.to_pretty()).map_err(|e| format!("{}: {}", sum_path, e))?;

    if !opts.quiet {
        print_report(&summary, &opponents);
    }
    outln!("wrote {} and {}", sum_path, ev_path);
    Ok(0)
}

fn params_json() -> Json {
    Json::obj()
        .with("values", Json::obj().with("P", VALUES[0]).with("N", VALUES[1]).with("B", VALUES[2]).with("R", VALUES[3]).with("Q", VALUES[4]))
        .with("queen", Json::obj().with("d2", QUEEN_D2).with("d4", QUEEN_D4))
        .with("exchange", Json::obj().with("d2", EXCH_D2).with("d4", EXCH_D4))
        .with("piece", Json::obj().with("d2", PIECE_D2).with("d4", PIECE_D4))
        .with("pawn", Json::obj().with("d2", PAWN_D).with("d4", PAWN_D).with("d6", PAWN_D))
        .with("intent_drop", INTENT_DROP)
        .with("mate_window_plies", MATE_WINDOW)
        .with("recoup_window_plies", RECOUP_WINDOW)
        .with("recoup_margin", RECOUP_MARGIN)
        .with("success_score_margin", SUCCESS_SCORE_MARGIN)
        .with("sf_later_offset_plies", SF_LATER_OFFSET)
        .with("sf_success", SF_SUCCESS)
        .with("unsound_drop", UNSOUND_DROP)
        .with("unsound_abs", UNSOUND_ABS)
        .with("practical_swing", PRACTICAL_SWING)
        .with("practical_window_plies", PRACTICAL_WINDOW)
        .with("sf_mate", SF_MATE)
        .with("trade_guard_margin", TRADE_GUARD_MARGIN)
        .with("context_cp", CONTEXT_CP)
}

fn counts_json(c: &Counts) -> Json {
    let mut j = Json::obj();
    for (i, ch) in PIECE_CH.iter().enumerate() {
        if c[i] != 0 {
            j.set(ch, c[i]);
        }
    }
    j
}

fn event_json(e: &Event, p: &Persp, pi: &PerspInfo, file: &Path) -> Json {
    let rec = p.rec;
    let k = e.k;
    let fm = k / 2 + 1;
    let san = &rec.san[k];
    let label = if k % 2 == 0 { format!("{}.{}", fm, san) } else { format!("{}...{}", fm, san) };
    let mut j = Json::obj()
        .with("type", e.kind.as_str())
        .with("category", e.cand.cat.as_str())
        .with("file", file.to_string_lossy().to_string())
        .with("game_id", rec.id)
        .with("pairing", rec.pairing)
        .with("game_pair", rec.game_pair)
        .with("opening_index", rec.opening_index)
        .with("white", &rec.white)
        .with("black", &rec.black)
        .with("engine", &pi.engine)
        .with("opponent", &pi.opponent)
        .with("color", if pi.color == Color::White { "white" } else { "black" })
        .with("ply", k)
        .with("move_number", fm)
        .with("move", rec.moves[k].as_str())
        .with("san", label)
        .with("book", e.book)
        .with("fen", format!("{}", p.rep.boards[k]))
        .with("line", p.san_line(k, k + 8))
        .with("bal", vec![p.bal(k), p.bal(k + 2), p.bal(k + 4), p.bal(k + 6)])
        .with("bal_k3", p.bal(k + 3))
        .with("bal_k5", p.bal(k + 5))
        .with("d3", e.cand.d3)
        .with("d5", e.cand.d5)
        .with("opponent_promotions", e.cand.opp_promotions)
        .with("mating", e.cand.mating)
        .with("legal_moves", e.legal_moves)
        .with("e_prev", e.e_prev.map(|x| x as i64))
        .with("eval_ply", e.eval_ply)
        .with("baseline_ply", e.cand.base)
        .with("bal_baseline", p.bal(e.cand.base))
        .with("in_check", !p.rep.boards[k].checkers().is_empty())
        .with("d2", e.cand.d2)
        .with("d4", e.cand.d4)
        .with("d6", e.cand.d6)
        .with("lost_engine", counts_json(&e.cand.lost_e))
        .with("lost_opponent", counts_json(&e.cand.lost_o))
        .with("net_lost", counts_json(&e.cand.net))
        .with("truncated", e.truncated)
        .with("e0", e.e0.map(|x| x as i64))
        .with("e1", e.e1.map(|x| x as i64))
        .with("intent", e.intent.as_str())
        .with("intent_unknown", e.intent == Intent::Unknown)
        .with("context", context(e.e0))
        .with("chain_len", e.chain_len)
        .with(
            "chain",
            Json::Arr(
                e.chain
                    .iter()
                    .map(|f| {
                        Json::obj()
                            .with("ply", f.ply)
                            .with("san", rec.san[f.ply].as_str())
                            .with("category", f.cat.as_str())
                            .with("intent", f.intent.as_str())
                    })
                    .collect(),
            ),
        )
        .with("result", p.result_str())
        .with("termination", rec.termination.as_str())
        .with("game_plies", p.n());
    if e.kind == Kind::Sacrifice {
        j.set("mate", e.mate_ply.is_some());
        j.set("mate_ply", e.mate_ply);
        j.set("recouped", e.recouped_ply.is_some());
        j.set("recouped_ply", e.recouped_ply);
        j.set("recoup_score", e.recoup_score.map(|x| x as i64));
        j.set("successful_attack", e.successful);
        j.set("success_reason", e.success_reason);
    }
    j.set("sf_before", e.sf_before.map(|x| x as i64));
    j.set("sf_after", e.sf_after.map(|x| x as i64));
    if e.kind == Kind::Sacrifice {
        j.set("sf_k12", e.sf_later.map(|x| x as i64));
        j.set("unsound", e.unsound);
        j.set("practical_chance", e.practical);
        match e.sf_best_later {
            Some((v, ply)) => {
                j.set("sf_best_later", v as i64);
                j.set("sf_best_later_ply", ply);
            }
            None => {
                j.set("sf_best_later", Json::Null);
                j.set("sf_best_later_ply", Json::Null);
            }
        }
    }
    j
}

// ---------------------------------------------------------------- text report

fn fmt_prop(j: Option<&Json>) -> String {
    let Some(j) = j else { return "-".into() };
    let c = j.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
    let n = j.get("of").and_then(|v| v.as_i64()).unwrap_or(0);
    match (j.get("rate").and_then(|v| v.as_f64()), j.get("ci95").and_then(|v| v.as_array())) {
        (Some(r), Some(ci)) if ci.len() == 2 => format!(
            "{}/{} = {:.1}% [{:.1}, {:.1}]",
            c,
            n,
            100.0 * r,
            100.0 * ci[0].as_f64().unwrap_or(0.0),
            100.0 * ci[1].as_f64().unwrap_or(0.0)
        ),
        _ => format!("{}/{}", c, n),
    }
}

fn fmt_mean(j: Option<&Json>, scale: f64) -> String {
    let Some(j) = j else { return "-".into() };
    match j.get("mean").and_then(|v| v.as_f64()) {
        None => "-".into(),
        Some(m) => match j.get("ci95").and_then(|v| v.as_array()) {
            Some(ci) if ci.len() == 2 => format!(
                "{:.2} [{:.2}, {:.2}]",
                m * scale,
                ci[0].as_f64().unwrap_or(0.0) * scale,
                ci[1].as_f64().unwrap_or(0.0) * scale
            ),
            _ => format!("{:.2}", m * scale),
        },
    }
}

fn print_block(name: &str, b: &Json) {
    let i = |k: &str| b.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
    let r = b.get("result");
    let ri = |k: &str| r.and_then(|r| r.get(k)).and_then(|v| v.as_i64()).unwrap_or(0);
    outln!("== {} ==", name);
    outln!(
        "  games {}  (+{} ={} -{}), score {}%",
        i("games"),
        ri("wins"),
        ri("draws"),
        ri("losses"),
        fmt_mean(r.and_then(|r| r.get("score")), 100.0)
    );
    outln!("  games with >=1 sacrifice: {}", fmt_prop(b.get("games_with_sacrifice")));
    outln!("  sacrifices: {}   per game: {}", i("sacrifices"), fmt_mean(b.get("sacrifices_per_game"), 1.0));
    let cats = b.get("categories");
    let ci = |k: &str| cats.and_then(|c| c.get(k)).and_then(|v| v.as_i64()).unwrap_or(0);
    outln!(
        "  categories: queen {}  exchange {}  piece {}  pawn {}   (book {}, intent unknown {}, chained {})",
        ci("queen"),
        ci("exchange"),
        ci("piece"),
        ci("pawn"),
        i("book_sacrifices"),
        i("intent_unknown"),
        i("chained_sacrifices")
    );
    let cx = b.get("sacrifices_by_context");
    let cxi = |k: &str| cx.and_then(|c| c.get(k)).and_then(|v| v.as_i64()).unwrap_or(0);
    outln!(
        "  context (engine eval): balanced {}  winning {}  losing {}  unknown {}   balanced per game: {}",
        cxi("balanced"),
        cxi("winning"),
        cxi("losing"),
        cxi("unknown"),
        fmt_mean(b.get("balanced_sacrifices_per_game"), 1.0)
    );
    outln!(
        "  blunders (filtered out): {}   per game: {}   (of which caused by the previous move: {})",
        i("blunders"),
        fmt_mean(b.get("blunders_per_game"), 1.0),
        i("blunders_collapsed")
    );
    outln!(
        "  excluded: forced material losses (single legal move) {}, moves while being mated {}",
        i("forced_losses"),
        i("desperation_moves")
    );
    outln!("  mate after sacrifice:  {}", fmt_prop(b.get("mates_after_sacrifice")));
    outln!("  recouped:              {}", fmt_prop(b.get("recouped")));
    outln!("  successful attacks:    {}", fmt_prop(b.get("successful_attacks")));
    if b.get("unsound").is_some() {
        outln!("  unsound (SF):          {}", fmt_prop(b.get("unsound")));
        outln!("  practical chances:     {}   (of unsound)", fmt_prop(b.get("practical_chances")));
        outln!("  blunders confirmed SF: {}", fmt_prop(b.get("blunders_confirmed_by_sf")));
    }
    let sw = b.get("score_games_with_sacrifice");
    let so = b.get("score_games_without_sacrifice");
    outln!(
        "  score with sacrifice: {}%   without: {}%",
        fmt_mean(sw.and_then(|r| r.get("score")), 100.0),
        fmt_mean(so.and_then(|r| r.get("score")), 100.0)
    );
    outln!("  avg move number of sacrifices: {}", fmt_mean(b.get("avg_move_number"), 1.0));
    if let Some(pc) = b.get("per_category") {
        for c in Category::ALL {
            if let Some(cb) = pc.get(c.as_str()) {
                if cb.get("count").and_then(|v| v.as_i64()).unwrap_or(0) > 0 {
                    let mut line = format!("    {:<8} success {}", c.as_str(), fmt_prop(cb.get("successful_attack")));
                    if cb.get("unsound").is_some() {
                        line.push_str(&format!("   unsound {}", fmt_prop(cb.get("unsound"))));
                    }
                    outln!("{}", line);
                }
            }
        }
    }
}

fn print_report(summary: &Json, opponents: &[String]) {
    let i = |k: &str| summary.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
    outln!(
        "{} games read, {} analysed perspectives ({} without the engine, {} invalid)",
        i("games_in_files"),
        i("games_analysed"),
        i("games_without_engine"),
        i("games_invalid")
    );
    if let Some(o) = summary.get("overall") {
        print_block("overall", o);
    }
    if let Some(po) = summary.get("per_opponent") {
        if opponents.len() > 1 {
            for o in opponents {
                if let Some(b) = po.get(o) {
                    print_block(&format!("vs {}", o), b);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{GameResult, Termination};

    fn rec(white: &str, black: &str, moves: &str, opening: usize, scores: Vec<Option<i32>>, book: &[usize], result: GameResult, term: Termination) -> GameRecord {
        let moves: Vec<String> = moves.split_whitespace().map(|s| s.to_string()).collect();
        let n = moves.len();
        let mut sc = scores;
        sc.resize(n, None);
        let mut bk = vec![false; n];
        for &b in book {
            bk[b] = true;
        }
        GameRecord {
            id: 1,
            white: white.into(),
            black: black.into(),
            pairing: 0,
            game_pair: 0,
            opening_index: 0,
            opening_plies: opening,
            san: moves.clone(),
            moves,
            scores: sc,
            depths: vec![None; n],
            times_ms: vec![0; n],
            book: bk,
            result,
            termination: term,
            detail: String::new(),
            tc: "test".into(),
            forfeit_side: None,
            clock_end_ms: None,
            date: String::new(),
        }
    }

    fn run_detect(r: &GameRecord, color: Color) -> Vec<Event> {
        let rep = Replay::new(&r.moves).expect("legal moves");
        let p = Persp { rec: r, rep: &rep, color, guards: true };
        detect(&p, 0, 0)
    }

    fn s(v: &[i32]) -> Vec<Option<i32>> {
        v.iter().map(|&x| if x == i32::MIN { None } else { Some(x) }).collect()
    }
    const N: i32 = i32::MIN;

    #[test]
    fn classify_patterns() {
        // net order P N B R Q
        assert_eq!(classify(800, 700, 700, &[-2, 0, 0, 0, 1], true), Some(Category::Queen));
        assert_eq!(classify(180, 180, 180, &[0, -1, 0, 1, 0], true), Some(Category::Exchange));
        assert_eq!(classify(80, 80, 80, &[1, -1, 0, 1, 0], true), None); // R for N+P: too small
        assert_eq!(classify(320, 320, 320, &[0, 1, 0, 0, 0], true), Some(Category::Piece));
        assert_eq!(classify(220, 150, 150, &[-1, 0, 1, 0, 0], true), Some(Category::Piece));
        assert_eq!(classify(220, 140, 140, &[-1, 0, 1, 0, 0], true), None);
        assert_eq!(classify(100, 100, 100, &[1, 0, 0, 0, 0], true), Some(Category::Pawn));
        assert_eq!(classify(100, 100, 50, &[1, 0, 0, 0, 0], true), None); // gambit returned at k+6
        assert_eq!(classify(10, 10, 10, &[0, 1, -1, 0, 0], true), None); // B for N trade
        // queen for rook + minor (80 cp) is not a sacrifice of any kind
        assert_eq!(classify(80, 80, 80, &[0, -1, 0, -1, 1], true), None);
        // queen for rook: queen category takes precedence
        assert_eq!(classify(400, 400, 400, &[0, 0, 0, -1, 1], true), Some(Category::Queen));
        // net queens down without the engine losing a queen (an opponent promotion that
        // was not neutralised) is never a queen sacrifice
        assert_eq!(classify(800, 800, 800, &[0, 0, 0, 0, 1], false), None);
    }

    #[test]
    fn judge_rules() {
        // desperation: the engine is being mated
        assert_eq!(judge(Some(-99_995), Some(-99_996), Some(-99_994), 3), Intent::Desperation);
        // forced: one legal move, no collapse
        assert_eq!(judge(Some(-40), Some(-60), Some(-30), 1), Intent::Forced);
        // forced and collapsed: blunder of the previous move
        assert_eq!(judge(Some(-450), Some(-440), Some(20), 1), Intent::Collapsed);
        // collapsed since the previous move, several legal moves
        assert_eq!(judge(Some(-300), Some(-290), Some(0), 5), Intent::Collapsed);
        // mate intent beats collapse
        assert_eq!(judge(Some(-300), Some(99_990), Some(0), 5), Intent::Mate);
        // otherwise the spec intent filter
        assert_eq!(judge(Some(50), Some(-90), Some(60), 20), Intent::Sound);
        assert_eq!(judge(Some(50), Some(-101), Some(60), 20), Intent::Blunder);
        assert_eq!(judge(None, Some(-500), None, 20), Intent::Unknown);
        assert!(Intent::Sound.is_sacrifice() && Intent::Mate.is_sacrifice() && Intent::Unknown.is_sacrifice());
        assert!(!Intent::Collapsed.is_sacrifice() && !Intent::Forced.is_sacrifice() && !Intent::Desperation.is_sacrifice());
    }

    #[test]
    fn fork_forced_loss_is_blunder_of_previous_move() {
        // 1.Nf3 Nc6 2.e3 Nb4 3.Qe2?? Nxc2+ 4.Kd1 (only move) Nxa1 5.b3 d6 6.Bb2 Bd7 7.Nc3 Nf6
        let r = rec(
            "aggro",
            "sf",
            "g1f3 b8c6 e2e3 c6b4 d1e2 b4c2 e1d1 c2a1 b2b3 d7d6 c1b2 c8d7 b1c3 g8f6",
            2,
            s(&[N, N, 20, -20, 20, 30, -450, 440, -440, 430, -430, 430, -430, 430]),
            &[],
            GameResult::BlackWins,
            Termination::Maxplies,
        );
        let ev = run_detect(&r, Color::White);
        assert_eq!(ev.len(), 1, "{:?}", ev);
        let e = &ev[0];
        assert_eq!((e.k, e.kind, e.intent, e.cand.cat, e.legal_moves), (6, Kind::Blunder, Intent::Collapsed, Category::Piece, 1));
        assert_eq!(e.eval_ply, 4); // judged at 3.Qe2
    }

    #[test]
    fn opponent_promotion_is_not_a_queen_sacrifice() {
        // real game (original AggroChess as White vs a modified build): ... 69.Kd1 a2 70.Ke1 a1=Q#.
        // White's own scores are replaced by -100 so that the desperation rule does not apply.
        let moves = "g1f3 g8f6 g2g3 g7g6 b2b3 f8g7 c1b2 d7d6 d2d4 c7c5 c2c4 e8g8 f1g2 d6d5 c4d5 f6d5 d1d2 d5c7                      f3g5 c5d4 g5h7 g8h7 h2h4 c7d5 h4h5 e7e5 b2a3 d5e3 h5g6 h7g6 f2e3 d8g5 a3f8 g5g3 e1f1 d4e3                      g2e4 g6g5 f8e7 f7f6 d2e1 c8h3 h1h3 g3h3 e4g2 h3f5 f1g1 f5g4 b1c3 b8c6 c3d5 c6e7 d5e7 a8h8                      e7d5 h8h3 a1c1 f6f5 c1c7 g7f8 c7f7 h3g3 e1f1 f8h6 b3b4 g4e4 g1h2 e4h4 g2h3 f5f4 f7f5 g5g6                      f5e5 h6g5 a2a3 g3g4 h2h1 b7b5 e5f5 g4g3 f5g5 g6g5 d5f4 h4f4 f1f4 g5f4 h3d7 a7a6 d7c8 g3g6                      h1h2 g6c6 c8b7 c6c2 h2g2 c2e2 g2f1 e2h2 f1e1 h2h7 b7d5 h7c7 e1e2 c7c3 e2e1 c3a3 d5b7 a3a1                      e1e2 a1a2 e2e1 f4e5 b7c8 e5d4 c8d7 a2a1 e1e2 a1b1 d7g4 b1b4 e2d1 b4b2 d1e1 a6a5 g4f3 a5a4                      e1d1 a4a3 d1c1 e3e2 f3e2 b2e2 c1b1 b5b4 b1c1 e2b2 c1d1 a3a2 d1e1 a2a1q";
        let n = moves.split_whitespace().count();
        assert_eq!(n, 140);
        let scores: Vec<i32> = (0..n).map(|i| if i < 16 { N } else if i % 2 == 0 { -100 } else { 100 }).collect();
        let r = rec("orig", "x", moves, 16, s(&scores), &[], GameResult::BlackWins, Termination::Checkmate);
        let rep = Replay::new(&r.moves).unwrap();
        let p = Persp { rec: &r, rep: &rep, color: Color::White, guards: true };
        assert_eq!(p.bal(138) - p.bal(140), 800); // the promotion costs White 800 ...
        assert_eq!(p.opp_promotions(138, 142)[4], 1);
        assert!(p.candidate(138).is_none()); // ... but 70.Ke1 gave up nothing
        assert!(run_detect(&r, Color::White).iter().all(|e| e.cand.cat != Category::Queen || e.cand.lost_e[4] >= 1));
    }

    #[test]
    fn opponent_initiated_trades_are_not_a_pawn_sacrifice() {
        // real game: 11...Qd8 12.Nxf6+ Bxf6 13.dxe5 Nxe5 14.Nxe5 Bxe5 - sampled after White's
        // moves the material is down 320/100/320, but Black recaptures every time
        let r = rec(
            "sf",
            "orig",
            "c2c4 g8f6 b1c3 g7g6 e2e4 d7d6 d2d4 f8g7 h2h3 e8g8 c1g5 a7a6 a2a4 c7c6 g1f3 b8d7 d1c1 d8a5              g5d2 e7e5 c3d5 a5d8 d5f6 g7f6 d4e5 d7e5 f3e5 f6e5 f1d3 f7f5 e1g1 f5f4 d2c3 c8h3",
            16,
            s(&[N, N, N, N, N, N, N, N, N, N, N, N, N, N, N, N, 33, 20, 59, 43, 186, 80, -23, 55, -75, 70, -92, 93, -50, 118, -10, 378, -50, 491]),
            &[],
            GameResult::WhiteWins,
            Termination::Maxplies,
        );
        let rep = Replay::new(&r.moves).unwrap();
        let raw = Persp { rec: &r, rep: &rep, color: Color::Black, guards: false };
        assert_eq!(raw.candidate(21).map(|c| c.cat), Some(Category::Pawn)); // the literal spec rule
        let p = Persp { rec: &r, rep: &rep, color: Color::Black, guards: true };
        assert_eq!(p.bal(24), p.bal(21)); // back to even after Black's own reply
        assert!(p.candidate(21).is_none()); // guard (c)
    }

    #[test]
    fn intent_rules() {
        assert_eq!(intent(Some(50), Some(-90)), Intent::Sound);
        assert_eq!(intent(Some(50), Some(-101)), Intent::Blunder);
        assert_eq!(intent(Some(50), Some(99_995)), Intent::Mate);
        assert_eq!(intent(Some(99_990), Some(-500)), Intent::Mate);
        assert_eq!(intent(None, Some(-500)), Intent::Unknown);
        assert_eq!(intent(Some(10), Some(-99_990)), Intent::Blunder);
    }

    #[test]
    fn queen_sacrifice_to_mate_white() {
        // Legal's mate: 1.e4 e5 2.Nf3 d6 3.Bc4 Bg4 4.Nc3 g6 5.Nxe5 Bxd1 6.Bxf7+ Ke7 7.Nd5#
        let r = rec(
            "aggro",
            "sf",
            "e2e4 e7e5 g1f3 d7d6 f1c4 c8g4 b1c3 g7g6 f3e5 g4d1 c4f7 e8e7 c3d5",
            4,
            s(&[N, N, N, N, 30, -20, 40, -30, 150, -400, 99_998, -99_999, 99_999]),
            &[],
            GameResult::WhiteWins,
            Termination::Checkmate,
        );
        let ev = run_detect(&r, Color::White);
        assert_eq!(ev.len(), 1, "{:?}", ev);
        let e = &ev[0];
        assert_eq!((e.k, e.kind, e.cand.cat), (8, Kind::Sacrifice, Category::Queen));
        assert_eq!((e.cand.d2, e.cand.d4), (800, 700));
        assert_eq!(e.intent, Intent::Mate);
        assert_eq!(e.mate_ply, Some(12));
        assert!(e.truncated);
        // black's perspective: no sacrifice
        assert!(run_detect(&r, Color::Black).is_empty());
    }

    #[test]
    fn exchange_sacrifice_black() {
        // 1.Nf3 h5 2.e3 Rh6 3.Ng5 Rg6 4.h4 Rxg5 5.hxg5 e6 6.d4 d6 7.Nc3 Nc6 8.Bd3 Nge7 9.Qe2 Bd7 10.Bd2 Qc8
        let r = rec(
            "sf",
            "aggro",
            "g1f3 h7h5 e2e3 h8h6 f3g5 h6g6 h2h4 g6g5 h4g5 e7e6 d2d4 d7d6 b1c3 b8c6 f1d3 g8e7 d1e2 c8d7 c1d2 d8c8",
            2,
            s(&[N, N, 0, -10, 0, -20, 0, 30, 0, 0, 0, 10, 0, -20, 0, -30, 0, -40, 0, -40]),
            &[],
            GameResult::Draw,
            Termination::Maxplies,
        );
        let ev = run_detect(&r, Color::Black);
        assert_eq!(ev.len(), 1, "{:?}", ev);
        let e = &ev[0];
        assert_eq!((e.k, e.kind, e.cand.cat), (7, Kind::Sacrifice, Category::Exchange));
        assert_eq!((e.cand.d2, e.cand.d4, e.cand.d6), (180, 180, 180));
        assert_eq!(e.intent, Intent::Sound);
        assert!(e.recouped_ply.is_none() && e.mate_ply.is_none());
        assert!(run_detect(&r, Color::White).is_empty());
    }

    #[test]
    fn book_pawn_gambit() {
        // King's Gambit: 1.e4 e5 2.f4 exf4 3.Nf3 d6 4.Bc4 h6 5.d4 g5 6.O-O Bg7 7.c3 Nc6
        let r = rec(
            "aggro",
            "sf",
            "e2e4 e7e5 f2f4 e5f4 g1f3 d7d6 f1c4 h7h6 d2d4 g7g5 e1g1 f8g7 c2c3 b8c6",
            2,
            s(&[N, N, N, 20, 10, -10, 20, -20, 30, -30, 30, -30, 30, -30]),
            &[2],
            GameResult::Draw,
            Termination::Maxplies,
        );
        let ev = run_detect(&r, Color::White);
        assert_eq!(ev.len(), 1, "{:?}", ev);
        let e = &ev[0];
        assert_eq!((e.k, e.cand.cat, e.book, e.intent), (2, Category::Pawn, true, Intent::Unknown));
    }

    #[test]
    fn plain_trades_not_detected() {
        // Ruy Lopez exchange with pawn and queen trades:
        // 1.e4 e5 2.Nf3 Nc6 3.Bb5 a6 4.Bxc6 dxc6 5.O-O f6 6.d4 exd4 7.Nxd4 c5 8.Nb3 Qxd1 9.Rxd1 Bg4 10.f3 Be6 11.Nc3 Bd6
        let r = rec(
            "aggro",
            "sf",
            "e2e4 e7e5 g1f3 b8c6 f1b5 a7a6 b5c6 d7c6 e1g1 f7f6 d2d4 e5d4 f3d4 c6c5 d4b3 d8d1 f1d1 c8g4 f2f3 g4e6 b1c3 f8d6",
            4,
            vec![Some(0); 22],
            &[],
            GameResult::Draw,
            Termination::Maxplies,
        );
        assert!(run_detect(&r, Color::White).is_empty());
        assert!(run_detect(&r, Color::Black).is_empty());
    }

    #[test]
    fn trade_guard_zwischenzug() {
        // From a real game (original AggroChess vs SF 1800): 10.Qxd5 Qxd5 11.Nf6+ gxf6 12.Bxd5.
        // The queen is recaptured by the engine's own move at k+4 after an in-between check,
        // so bal(k+2) and bal(k+4) both show a deficit but the material is back at k+5.
        let r = rec(
            "aggro",
            "sf",
            "c2c4 e7e5 b1c3 g8f6 g2g3 c7c6 f1g2 d7d5 c4d5 c6d5 d2d3 d5d4 c3e4 f6d5 d1b3 f8b4 \
             e1f1 a7a5 b3d5 d8d5 e4f6 g7f6 g2d5 e8e7 a2a3 b4c5 h2h4 c8e6",
            16,
            s(&[N, N, N, N, N, N, N, N, N, N, N, N, N, N, N, N, -3, 123, 220, 43, -102, 39, -146, 30, -95, 42, -80, 43]),
            &[],
            GameResult::WhiteWins,
            Termination::Maxplies,
        );
        let rep = Replay::new(&r.moves).unwrap();
        let p = Persp { rec: &r, rep: &rep, color: Color::White, guards: true };
        // the spec deficits alone would make ply 18 (10.Qxd5) a queen-category candidate ...
        assert_eq!((p.bal(18), p.bal(20), p.bal(22), p.bal(23)), (0, -580, -900, 0));
        let raw = Persp { rec: &r, rep: &rep, color: Color::White, guards: false };
        assert_eq!(raw.candidate(18).map(|c| c.cat), Some(Category::Queen));
        // ... but trade guard (a) rejects it
        assert!(p.candidate(18).is_none());
        assert!(run_detect(&r, Color::White).is_empty());
        // Black's 11...gxf6 (in check, bal(k) inflated to +580 while White's Bxd5 is
        // pending) would be a queen "sacrifice" without the check-adjusted baseline (b)
        let rawb = Persp { rec: &r, rep: &rep, color: Color::Black, guards: false };
        assert_eq!(rawb.candidate(21).map(|c| c.cat), Some(Category::Queen));
        let pb = Persp { rec: &r, rep: &rep, color: Color::Black, guards: true };
        assert_eq!(pb.baseline(21), 19);
        assert!(pb.candidate(21).is_none());
        assert!(run_detect(&r, Color::Black).is_empty());
    }

    #[test]
    fn hung_piece_is_blunder() {
        // 1.e4 e5 2.Nf3 Nc6 3.Ng5?? Qxg5 4.d4 Qe7 5.d5 Nd4 6.Bd3 Nf6 7.O-O d6
        let r = rec(
            "aggro",
            "sf",
            "e2e4 e7e5 g1f3 b8c6 f3g5 d8g5 d2d4 g5e7 d4d5 c6d4 f1d3 g8f6 e1g1 d7d6",
            2,
            s(&[N, N, 20, -20, 30, 300, -300, 310, -310, 320, -320, 330, -330, 340]),
            &[],
            GameResult::BlackWins,
            Termination::Maxplies,
        );
        let ev = run_detect(&r, Color::White);
        assert_eq!(ev.len(), 1, "{:?}", ev);
        assert_eq!((ev[0].k, ev[0].kind, ev[0].cand.cat), (4, Kind::Blunder, Category::Piece));
    }

    #[test]
    fn opening_plies_excluded() {
        // the same gambit, but 2.f4 comes from the opening file
        let r = rec(
            "aggro",
            "sf",
            "e2e4 e7e5 f2f4 e5f4 g1f3 d7d6 f1c4 h7h6 d2d4 g7g5 e1g1 f8g7 c2c3 b8c6",
            4,
            vec![None; 14],
            &[],
            GameResult::Draw,
            Termination::Maxplies,
        );
        assert!(run_detect(&r, Color::White).is_empty());
    }

    #[test]
    fn wilson_ci() {
        let (lo, hi) = wilson(5, 10).unwrap();
        assert!((lo - 0.2366).abs() < 1e-3 && (hi - 0.7634).abs() < 1e-3);
        let (lo, hi) = wilson(0, 20).unwrap();
        assert!(lo == 0.0 && (hi - 0.1611).abs() < 1e-3);
    }
}
