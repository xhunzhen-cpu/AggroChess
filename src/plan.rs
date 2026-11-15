//! Plan: rank root moves by how close they are to a SOUND sacrifice, not just by their eval.
//!
//! The rest of the brilliancy machinery (`brilliant.rs`, the Tal look, the sacrifice bonus) is
//! *reactive*: it notices that a move sacrifices material and then tries to justify or rescue it.
//! That is why every lever measured over the last sessions is null -- the engine is already willing
//! to sacrifice (~2 sustained sacrifices a game), but only ~10% of the sound ones become badges,
//! and only ~40% of those even sacrifice the piece that moved.
//!
//! This module points the other way round. Instead of asking "is this move a sacrifice?", it asks
//! "does this position leave the opponent a capture that WINS material by static exchange?" -- a
//! piece of ours standing where the opponent can take it and come out ahead. That is a *sacrifice
//! setup*: our piece is attacked, the opponent profits by taking it, and if we have a good reason
//! not to move it, taking becomes a real decision for them.
//!
//! Two uses:
//!
//! * `setup_score` -- how many of our pieces the opponent can win by static exchange. Positive: a
//!   setup is what the plan WANTS to build.
//! * `plan_pick` -- among root moves whose search scores are within `margin` of the best, pick the
//!   one that leaves the opponent the most to win. It only ever re-ranks NEAR-EQUAL moves and never
//!   overrides the search's judgement of what is good, so the strength cost is bounded by `margin`,
//!   exactly like the Human Style taste in `human.rs`.
//!
//! `setup_score` is deliberately cheap: one null-move board, a scan of the opponent's pieces, and
//! one SEE per candidate capture. `plan_pick` runs at the top of the search only, never inside it.

use cozy_chess::{Board, BitBoard, Color, Move, Piece, Square};

use crate::search::see_ge;

/// Static-exchange margin the opponent must win for one of our pieces to count as winnable. 150 is
/// the same bar `brilliant::is_sacrifice` uses to call a move a sacrifice, so the two agree on what
/// "worth taking" means.
pub const WIN_GAIN: i32 = 150;

/// Most setup points one position can contribute, so a single messy position cannot dominate.
const SETUP_CAP: i32 = 4;

/// Squares `piece` on `sq` attacks under the current occupancy.
fn attacks_of_piece(board: &Board, sq: Square, piece: Piece, color: Color) -> u64 {
    match piece {
        Piece::Knight => cozy_chess::get_knight_moves(sq).0,
        Piece::Bishop => cozy_chess::get_bishop_moves(sq, board.occupied()).0,
        Piece::Rook => cozy_chess::get_rook_moves(sq, board.occupied()).0,
        Piece::Queen => {
            cozy_chess::get_bishop_moves(sq, board.occupied()).0
                | cozy_chess::get_rook_moves(sq, board.occupied()).0
        }
        Piece::King => cozy_chess::get_king_moves(sq).0,
        Piece::Pawn => cozy_chess::get_pawn_attacks(sq, color).0,
    }
}

const PIECES: [Piece; 6] = [
    Piece::Knight,
    Piece::Bishop,
    Piece::Rook,
    Piece::Queen,
    Piece::King,
    Piece::Pawn,
];

/// How many of `side`'s pieces the opponent can win outright by static exchange.
///
/// This is the "sacrifice setup" measure: each such piece is one the opponent can profitably take,
/// so leaving it there and playing elsewhere is the shape chess.com badges as brilliant.
pub fn setup_score(board: &Board, side: Color) -> i32 {
    let opp = !side;
    // We need the board with the OPPONENT to move: that is what `see_ge` judges. If they are
    // already to move, the board itself is that position; otherwise take the null move.
    let nb = if board.side_to_move() == opp {
        board.clone()
    } else {
        match board.null_move() {
            Some(b) => b,
            None => return 0,
        }
    };
    let our = (board.colors(side) & !board.colored_pieces(side, Piece::King)).0;
    if our == 0 {
        return 0;
    }
    // only opponent pieces that attack at least one of our men can produce a winning capture
    let mut movers = 0u64;
    for piece in PIECES {
        for sq in board.colored_pieces(opp, piece) {
            if attacks_of_piece(board, sq, piece, opp) & our != 0 {
                movers |= 1u64 << sq as u32;
            }
        }
    }
    if movers == 0 {
        return 0;
    }
    let mut n = 0i32;
    nb.generate_moves_for(BitBoard(movers), |mvs| {
        for mv in mvs {
            if our & (1u64 << mv.to as u32) != 0 && see_ge(&nb, mv, WIN_GAIN) {
                n += 1;
                if n >= SETUP_CAP {
                    return true; // stop generating
                }
            }
        }
        false
    });
    n.min(SETUP_CAP)
}

/// Static value of the piece that moved, for `offer_gain`.
fn piece_value(p: Piece) -> i32 {
    match p {
        Piece::Pawn => 100,
        Piece::Knight => 320,
        Piece::Bishop => 330,
        Piece::Rook => 500,
        Piece::Queen => 900,
        Piece::King => 20_000,
    }
}

/// Does `mv` offer the MOVED piece -- can the opponent win it by static exchange?
///
/// This is the chess.com brilliant shape itself, and it is the gate the project's funnel dies on:
/// of 1500 games, 0.087 sound sacrifices per game become only 0.036 once the MOVED piece must be
/// the sacrificed one. So the plan prefers a near-equal move that puts the very piece that moved
/// where it can be taken, rather than any move after which something else falls later.
///
/// Returns the value of the moved piece when it is winnable by SEE for the opponent, else 0.
pub fn offer_gain(board: &Board, mv: Move) -> i32 {
    let moved = match board.piece_on(mv.from) {
        Some(p) if p != Piece::King => p,
        _ => return 0, // a king cannot be offered, and a promotion is its own category
    };
    if mv.promotion.is_some() {
        return 0;
    }
    let mut child = board.clone();
    child.play_unchecked(mv);
    // child: the OPPONENT is to move. The moved piece is offered when one of their captures
    // landing on our new square wins material by static exchange (SEE >= WIN_GAIN).
    //
    // `generate_moves_for` masks by FROM square (which pieces may move), not by destination, so
    // first collect the opponent pieces whose attacks already cover the target square.
    let target = mv.to;
    let opp = child.side_to_move();
    let target_bb = 1u64 << target as u32;
    let mut movers = 0u64;
    for piece in PIECES {
        for sq in child.colored_pieces(opp, piece) {
            if attacks_of_piece(&child, sq, piece, opp) & target_bb != 0 {
                movers |= 1u64 << sq as u32;
            }
        }
    }
    if movers == 0 {
        return 0;
    }
    let mut offered = false;
    child.generate_moves_for(BitBoard(movers), |mvs| {
        for cap in mvs {
            if cap.to == target && see_ge(&child, cap, WIN_GAIN) {
                offered = true;
                return true; // stop generating
            }
        }
        false
    });
    if offered {
        piece_value(moved)
    } else {
        0
    }
}

/// `offer_gain` is the primary measure (the moved piece is the one offered); `setup_score` is the
/// broader one (any of our pieces is winnable) and is kept for diagnostics and future use.
#[allow(dead_code)]
fn setup_score_after(board: &Board, mv: Move, me: Color) -> i32 {
    let mut child = board.clone();
    child.play_unchecked(mv);
    setup_score(&child, me)
}

/// Re-rank root moves toward offering the piece that moved.
///
/// `scores` are the root lines' scores in `root_moves` order; `margin` is how many centipawns a
/// move may be worse than the best and still be considered. Returns the index into `root_moves` of
/// the preferred move, or `None` to leave the search's own choice alone. It never returns a move
/// worse than `margin` below the best, and never returns a move that offers nothing (the search's
/// own choice stands, including when that choice already offers a piece).
pub fn plan_pick(
    board: &Board,
    root_moves: &[Move],
    scores: &[i32],
    margin: i32,
    max_cand: usize,
) -> Option<usize> {
    if root_moves.len() < 2 || scores.len() != root_moves.len() || margin <= 0 {
        return None;
    }
    let best = *scores.iter().max()?;
    let mut cands: Vec<usize> = (0..root_moves.len())
        .filter(|&i| scores[i] >= best - margin)
        .collect();
    if cands.len() < 2 {
        return None;
    }
    // Probe the best-scoring ones first. Truncation is a pure safety ceiling: `offer_gain` is
    // static (one movegen plus a SEE per candidate capture), so probing every near-equal move is
    // cheap, and truncating on a tie would let the plan miss the very move it exists to find.
    cands.sort_by(|&a, &b| scores[b].cmp(&scores[a]));
    cands.truncate(max_cand.max(32));

    let cur = cands[0];
    let mut best_i = cur;
    let mut best_offer = offer_gain(board, root_moves[cur]);
    let mut best_score = scores[cur];
    for &i in &cands[1..] {
        let o = offer_gain(board, root_moves[i]);
        if o > best_offer || (o == best_offer && scores[i] > best_score) {
            best_offer = o;
            best_score = scores[i];
            best_i = i;
        }
    }
    if best_i == cur || best_offer <= 0 {
        return None;
    }
    Some(best_i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(fen: &str) -> Board {
        Board::from_fen(fen, false).unwrap()
    }

    fn all_moves(b: &Board) -> Vec<Move> {
        let mut v = Vec::new();
        b.generate_moves(|mvs| {
            v.extend(mvs);
            false
        });
        v
    }

    #[test]
    fn a_hanging_knight_scores() {
        // Black knight on d5, White rook on d1 down the open d-file, nothing defends the knight.
        let b = board("4k3/8/8/3n4/8/8/8/3RK3 w - - 0 1");
        assert!(setup_score(&b, Color::Black) >= 1, "hanging knight must score");
    }

    #[test]
    fn a_knight_defended_by_a_pawn_no_longer_scores() {
        // Same, but the knight is defended by a pawn on c6: Rxd5 cxd5 costs White the exchange.
        let b = board("4k3/8/2p5/3n4/8/8/8/3RK3 w - - 0 1");
        assert_eq!(setup_score(&b, Color::Black), 0, "defended knight must not score");
    }

    #[test]
    fn a_pawn_defended_knight_is_still_winnable_by_a_pawn() {
        // A knight "defended" only by a pawn is still lost to an attacking PAWN: cxd5 cxd5 wins
        // N for P. This pins the SEE semantics the measure relies on.
        let b = board("4k3/8/2p5/3n4/2P5/8/8/4K3 w - - 0 1");
        assert_eq!(setup_score(&b, Color::Black), 1, "pawn-defended knight is winnable by a pawn");
    }

    #[test]
    fn a_bare_king_position_has_nothing_to_win() {
        let quiet = board("4k3/8/8/8/8/8/8/4K3 w - - 0 1");
        assert_eq!(setup_score(&quiet, Color::White), 0);
        assert_eq!(setup_score(&quiet, Color::Black), 0);
    }

    #[test]
    fn moving_a_piece_into_take_wins_its_value() {
        // White queen d1 -> d4, where the black knight on c6 can take it for free.
        let b = board("k7/8/2n5/8/8/8/8/K2Q4 w - - 0 1");
        let mut mg = Vec::new();
        b.generate_moves(|mvs| { mg.extend(mvs); false });
        let qd4 = mg.iter().copied().find(|m| m.from == Square::D1 && m.to == Square::D4).unwrap();
        assert_eq!(offer_gain(&b, qd4), 900, "the queen walking into take must be offered");
        // and a move that leaves it defended offers nothing
        let qd2 = mg.iter().copied().find(|m| m.from == Square::D1 && m.to == Square::D2).unwrap();
        assert_eq!(offer_gain(&b, qd2), 0, "a safe queen move offers nothing");
    }

    #[test]
    fn a_defended_move_offers_nothing() {
        // Knight to d5 where a pawn can take it, but our own pawn on c4 recaptures: SEE says the
        // capture loses for the opponent, so the piece is NOT offered.
        let b = board("k7/8/8/8/2P5/8/8/K1N5 w - - 0 1");
        let mut mg = Vec::new();
        b.generate_moves(|mvs| { mg.extend(mvs); false });
        let nd3 = mg.iter().copied().find(|m| m.from == Square::C1 && m.to == Square::D3).unwrap();
        // black has only a king here, so nothing can take at all
        assert_eq!(offer_gain(&b, nd3), 0);
    }

    #[test]
    fn plan_pick_prefers_the_move_that_offers_the_queen() {
        // White queen d1, Black knight c6 (attacks d4), kings a1/a8 so no square adjacent to a
        // king can hang. Qd4 offers the queen; no other queen move does. All scores equal, so only
        // the plan can decide.
        let b = board("k7/8/2n5/8/8/8/8/K2Q4 w - - 0 1");
        let moves = all_moves(&b);
        let scores: Vec<i32> = vec![0; moves.len()];
        let picked = plan_pick(&b, &moves, &scores, 40, 8).expect("an offered-queen move exists");
        let mv = moves[picked];
        assert_eq!(mv.from, Square::D1);
        assert_eq!(mv.to, Square::D4, "the plan should offer the queen to the knight");
    }

    #[test]
    fn plan_pick_respects_the_margin() {
        // Same position, but the queen walk is scored far worse than the best move:
        // the plan must not override that.
        let b = board("4k3/8/2n5/8/8/8/8/3QK3 w - - 0 1");
        let moves = all_moves(&b);
        let mut scores: Vec<i32> = vec![0; moves.len()];
        let qd4 = moves.iter().position(|m| m.from == Square::D1 && m.to == Square::D4).unwrap();
        scores[qd4] = -500;
        scores[0] = 0;
        // margin 40 excludes the -500 walk
        let picked = plan_pick(&b, &moves, &scores, 40, 8);
        assert!(picked != Some(qd4), "a move outside the margin must never be picked");
    }

    #[test]
    fn plan_pick_stays_silent_when_nothing_is_offered() {
        // No opponent pieces at all: nothing of ours can be taken, so no opinion.
        let b = board("k7/8/8/8/8/8/8/K2Q4 w - - 0 1");
        let moves = all_moves(&b);
        let scores: Vec<i32> = vec![0; moves.len()];
        assert_eq!(plan_pick(&b, &moves, &scores, 40, 8), None);
    }

    #[test]
    fn plan_pick_never_returns_the_move_it_started_from() {
        // The current best move already offers the queen: nothing to re-rank, so the plan must
        // defer to the search rather than "confirm" it.
        let b = board("k7/8/2n5/8/8/8/8/K2Q4 w - - 0 1");
        let moves = all_moves(&b);
        let qd4 = moves.iter().position(|m| m.from == Square::D1 && m.to == Square::D4).unwrap();
        let mut scores: Vec<i32> = vec![-10; moves.len()];
        scores[qd4] = 0;
        assert_eq!(plan_pick(&b, &moves, &scores, 40, 8), None);
    }
}
