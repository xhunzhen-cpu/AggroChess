//! Brilliancy: which root moves count as *brilliant*, for the `Brilliance` option.
//!
//! A move is a brilliancy candidate when it is one of
//!
//! * a **material sacrifice**: the moved piece is lost by static exchange (a minor piece for
//!   pawns, the exchange or more), or the move leaves something worth more than two pawns
//!   hanging. That covers the Tal/Qxh7+ kind, capturing (`Rxh7+!`) or quiet (`Qh6!!`), the
//!   exchange sacrifice, and the **desperado** (a capture whose piece cannot be saved: it is
//!   a sacrifice by the same static test);
//! * an **underpromotion**: a knight, bishop or rook promotion (Saavedra's `c8=R!!`). The
//!   usual search orders these last and the quiescence search never looks at them, so they
//!   need to be nominated here or they are never played;
//! * (implicitly) an **only move**: a candidate that the neutral look finds far better than
//!   every non-candidate move in a bad position is the only way to hold - that is decided by
//!   the look's scores, not by a separate pattern test.
//!
//! Nothing in this module searches. It is the cheap static classification; the judgement of
//! whether a nominated move is *sound* belongs to the neutral look in `search.rs`, which is
//! the only judge that can tell a brilliancy from a blunder.

use cozy_chess::{Board, Move, Piece};

/// Smallest look budget that can say anything at all.
pub const MIN_LOOK_NODES: u64 = 96;

/// The verification budget a weakened engine's look gets, in nodes at Brilliance 100. A level's
/// own budget is far too small to judge a sacrifice (level 5 searches ~600 nodes at depth 4,
/// and Saavedra's stalemate point is nine plies away), so the look runs on a small budget of
/// its own instead: a few thousand nodes, a few milliseconds, and completely separate from the
/// level's search - the level's own move and measured strength are untouched.
pub const VERIFY_NODES: u64 = 30_000;

/// How deep the verification look may go; the node budget is what normally stops it.
pub const VERIFY_DEPTH: i32 = 14;

/// Most the look may spend at a weakened engine.
pub const MAX_LOOK_NODES: u64 = 60_000;

/// Smallest time the look is worth starting at a weakened engine.
pub const MIN_LOOK_TIME_MS: u64 = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// material given away by the move itself or left hanging behind it
    Sacrifice,
    /// a knight, bishop or rook promotion
    Underpromotion,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Sacrifice => "sacrifice",
            Kind::Underpromotion => "underpromotion",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Candidate {
    pub mv: Move,
    pub kind: Kind,
}

/// What the brilliancy look may spend. A weakened engine's allowance is counted in nodes
/// (levels 1-18 have node budgets), a timed engine's in wall-clock time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Budget {
    Off,
    Nodes(u64),
    TimeMs(u64),
}

impl Budget {
    pub fn is_off(self) -> bool {
        matches!(self, Budget::Off)
    }
}

/// A knight, bishop or rook promotion.
pub fn is_underpromotion(mv: Move) -> bool {
    matches!(mv.promotion, Some(p) if p != Piece::Queen)
}

/// Does `m` give material away? The moved piece is lost by static exchange (two pawns or
/// more), or the move leaves something worth 250 or more hanging. Promotions are handled by
/// their own class: a queen promotion is not a sacrifice even when the queen is taken.
pub fn is_sacrifice(board: &Board, m: Move) -> bool {
    if board.piece_on(m.from) == Some(Piece::King) || m.promotion.is_some() {
        return false;
    }
    if !crate::search::see_ge(board, m, -150) {
        return true;
    }
    let mut child = board.clone();
    child.play_unchecked(m);
    crate::search::leaves_material_at(&child, 250)
}

/// The brilliancy candidates among the legal root moves, in move order.
pub fn candidates(board: &Board, root_moves: &[Move]) -> Vec<Candidate> {
    let mut out = Vec::new();
    for &m in root_moves {
        if is_underpromotion(m) {
            out.push(Candidate { mv: m, kind: Kind::Underpromotion });
        } else if is_sacrifice(board, m) {
            out.push(Candidate { mv: m, kind: Kind::Sacrifice });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use cozy_chess::Board;

    fn moves(fen: &str) -> (Board, Vec<Move>) {
        let b = Board::from_fen(fen, false).unwrap();
        let mut v = Vec::new();
        b.generate_moves(|mvs| {
            v.extend(mvs);
            false
        });
        (b, v)
    }

    #[test]
    fn knight_promotion_is_nominated_and_queen_is_not() {
        // a pawn on b7 can promote; only the non-queen promotions are nominees
        let (b, v) = moves("4k3/1P6/8/8/8/8/8/4K3 w - - 0 1");
        let c = candidates(&b, &v);
        let promo = |p: Piece| v.iter().copied().find(|m| m.promotion == Some(p)).unwrap();
        assert!(c.iter().any(|x| x.mv == promo(Piece::Knight) && x.kind == Kind::Underpromotion));
        assert!(c.iter().any(|x| x.mv == promo(Piece::Rook) && x.kind == Kind::Underpromotion));
        assert!(c.iter().any(|x| x.mv == promo(Piece::Bishop) && x.kind == Kind::Underpromotion));
        assert!(!c.iter().any(|x| x.mv == promo(Piece::Queen)), "the queen promotion is not brilliant");
        // a quiet king move is neither
        assert!(!c.iter().any(|x| x.kind == Kind::Sacrifice));
    }

    #[test]
    fn saavedra_rook_promotion_is_nominated_along_with_the_pawn_offer() {
        // Saavedra: 1.c8=R! (not Q: Qf8?? would be mate-worthy but c8=Q loses to Rc4+) and the
        // rook promotion is the underpromotion class; the quiet king moves are neither class.
        let (b, v) = moves("8/2P5/8/8/3r4/8/2K5/k7 w - - 0 5");
        let c = candidates(&b, &v);
        let rook_promo = v.iter().copied().find(|m| m.promotion == Some(Piece::Rook)).unwrap();
        assert!(c.iter().any(|x| x.mv == rook_promo && x.kind == Kind::Underpromotion));
        for m in v.iter().copied().filter(|m| b.piece_on(m.from) == Some(Piece::King)) {
            assert!(!c.iter().any(|x| x.mv == m), "a quiet king move is not brilliant");
        }
    }

    #[test]
    fn queen_sacrifice_on_the_castled_king_is_nominated() {
        // the Classic Greek gift: Bxh7+ gives the bishop for two pawns and opens the king
        let (b, v) = moves("r1bq1rk1/pppn1ppp/4p3/3pP3/1b1P4/2NB1N2/PPP2PPP/R1BQK2R w KQ - 0 8");
        let c = candidates(&b, &v);
        let bxh7 = v.iter().copied().find(|m| m.from == cozy_chess::Square::D3 && m.to == cozy_chess::Square::H7);
        assert!(bxh7.is_some());
        assert!(c.iter().any(|x| Some(x.mv) == bxh7), "Bxh7+ should be a sacrifice candidate");
    }
}
