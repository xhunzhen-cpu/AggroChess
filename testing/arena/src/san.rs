//! SAN formatting / parsing and UCI helpers on top of cozy-chess.
//!
//! cozy-chess encodes castling as "king captures own rook" (e1h1); use
//! `uci_string` / `parse_uci` to convert to/from standard UCI (e1g1).

use cozy_chess::util::{display_uci_move, parse_uci_move};
use cozy_chess::{Board, Color, Move, Piece, Square};

pub fn legal_moves(board: &Board) -> Vec<Move> {
    let mut v = Vec::with_capacity(64);
    board.generate_moves(|mvs| {
        v.extend(mvs);
        false
    });
    v
}

pub fn has_legal_move(board: &Board) -> bool {
    board.generate_moves(|_| true)
}

/// Standard UCI text for a cozy-chess move in `board` (castling as e1g1).
pub fn uci_string(board: &Board, mv: Move) -> String {
    display_uci_move(board, mv).to_string()
}

/// Parse standard UCI text into a *legal* move (None if unparsable or illegal).
/// Only standard notation is accepted: the move must print back as exactly the same
/// text, so Chess960-style castling (`e1h1`, king takes own rook — cozy-chess's
/// internal encoding) is rejected in a standard game, as standard GUIs do.
pub fn parse_uci(board: &Board, s: &str) -> Option<Move> {
    let s = s.trim();
    if s.len() < 4 || s.len() > 5 {
        return None;
    }
    let mv = parse_uci_move(board, s).ok()?;
    // reject e.g. "e7e8k" which cozy parses as a non-promotion
    if s.len() == 5 && mv.promotion.is_none() {
        return None;
    }
    if board.is_legal(mv) && display_uci_move(board, mv).to_string() == s {
        Some(mv)
    } else {
        None
    }
}

/// For a move string that `parse_uci` rejected: a hint if it is a legal castling move
/// written in king-takes-rook (Chess960) notation instead of standard UCI.
pub fn nonstandard_castle_hint(board: &Board, s: &str) -> Option<String> {
    let mv = parse_uci_move(board, s.trim()).ok()?;
    if board.is_legal(mv) && is_castle(board, mv) {
        let std = display_uci_move(board, mv).to_string();
        if std != s.trim() {
            return Some(format!("castling in king-takes-rook notation; standard UCI is '{}'", std));
        }
    }
    None
}

pub fn is_castle(board: &Board, mv: Move) -> bool {
    board.piece_on(mv.from) == Some(Piece::King) && board.color_on(mv.to) == Some(board.side_to_move())
}

pub fn is_capture(board: &Board, mv: Move) -> bool {
    if is_castle(board, mv) {
        return false;
    }
    board.color_on(mv.to) == Some(!board.side_to_move())
        || (board.piece_on(mv.from) == Some(Piece::Pawn) && mv.from.file() != mv.to.file())
}

pub fn piece_letter(p: Piece) -> char {
    match p {
        Piece::Pawn => 'P',
        Piece::Knight => 'N',
        Piece::Bishop => 'B',
        Piece::Rook => 'R',
        Piece::Queen => 'Q',
        Piece::King => 'K',
    }
}

fn file_char(sq: Square) -> char {
    (b'a' + sq.file() as u8) as char
}

fn rank_char(sq: Square) -> char {
    (b'1' + sq.rank() as u8) as char
}

/// SAN without check/mate suffix. `legal` must be the legal moves of `board`.
pub fn san_core(board: &Board, mv: Move, legal: &[Move]) -> String {
    let piece = match board.piece_on(mv.from) {
        Some(p) => p,
        None => return "??".to_string(),
    };
    if is_castle(board, mv) {
        return if mv.to.file() > mv.from.file() { "O-O".into() } else { "O-O-O".into() };
    }
    let capture = is_capture(board, mv);
    let mut s = String::with_capacity(8);
    if piece == Piece::Pawn {
        if capture {
            s.push(file_char(mv.from));
            s.push('x');
        }
        s.push(file_char(mv.to));
        s.push(rank_char(mv.to));
        if let Some(p) = mv.promotion {
            s.push('=');
            s.push(piece_letter(p));
        }
        return s;
    }
    s.push(piece_letter(piece));
    let mut ambiguous = false;
    let mut same_file = false;
    let mut same_rank = false;
    for m in legal {
        if m.to == mv.to && m.from != mv.from && board.piece_on(m.from) == Some(piece) && !is_castle(board, *m) {
            ambiguous = true;
            if m.from.file() == mv.from.file() {
                same_file = true;
            }
            if m.from.rank() == mv.from.rank() {
                same_rank = true;
            }
        }
    }
    if ambiguous {
        if !same_file {
            s.push(file_char(mv.from));
        } else if !same_rank {
            s.push(rank_char(mv.from));
        } else {
            s.push(file_char(mv.from));
            s.push(rank_char(mv.from));
        }
    }
    if capture {
        s.push('x');
    }
    s.push(file_char(mv.to));
    s.push(rank_char(mv.to));
    s
}

/// "+" / "#" / "" for the position after `mv`.
pub fn check_suffix(board: &Board, mv: Move) -> &'static str {
    let mut after = board.clone();
    after.play_unchecked(mv);
    if after.checkers().is_empty() {
        ""
    } else if has_legal_move(&after) {
        "+"
    } else {
        "#"
    }
}

/// Full SAN including check (+) / mate (#) suffix. `mv` must be legal.
pub fn san(board: &Board, mv: Move) -> String {
    let legal = legal_moves(board);
    let mut s = san_core(board, mv, &legal);
    s.push_str(check_suffix(board, mv));
    s
}

/// Strip annotations / check marks, normalise castling zeros.
fn normalise_san(tok: &str) -> String {
    let mut t = tok.trim().to_string();
    if let Some(stripped) = t.strip_suffix("e.p.") {
        t = stripped.trim_end().to_string();
    }
    while let Some(c) = t.chars().last() {
        if matches!(c, '+' | '#' | '!' | '?') {
            t.pop();
        } else {
            break;
        }
    }
    if t.starts_with('0') {
        t = t.replace('0', "O");
    }
    t
}

/// Parse a SAN token (as found in PGN movetext) into a legal move.
///
/// Primary method (as specified): generate legal moves, format each as SAN
/// and compare. Fallback for non-canonical SAN (over-disambiguation, missing
/// `=` or `x`): cozy-chess' structural SAN parser, which only returns legal moves.
pub fn parse_san(board: &Board, tok: &str) -> Option<Move> {
    let t = normalise_san(tok);
    if t.is_empty() {
        return None;
    }
    let legal = legal_moves(board);
    for &m in &legal {
        if san_core(board, m, &legal) == t {
            return Some(m);
        }
    }
    let m = cozy_chess::util::parse_san_move(board, &t).ok()?;
    if board.is_legal(m) {
        Some(m)
    } else {
        None
    }
}

/// True if `color` has at most a king plus one minor piece (no pawns), i.e.
/// cannot possibly deliver mate (used for the time-forfeit draw rule).
pub fn cannot_mate(board: &Board, color: Color) -> bool {
    let ours = board.colors(color);
    let heavy = board.pieces(Piece::Pawn) | board.pieces(Piece::Rook) | board.pieces(Piece::Queen);
    if !(ours & heavy).is_empty() {
        return false;
    }
    let minors = ours & (board.pieces(Piece::Knight) | board.pieces(Piece::Bishop));
    minors.len() <= 1
}

/// Dead-position draw by insufficient material: KvK, K+minor vs K,
/// K+B(s) vs K+B(s) with all bishops on one square colour.
pub fn insufficient_material(board: &Board) -> bool {
    let heavy = board.pieces(Piece::Pawn) | board.pieces(Piece::Rook) | board.pieces(Piece::Queen);
    if !heavy.is_empty() {
        return false;
    }
    let knights = board.pieces(Piece::Knight);
    let bishops = board.pieces(Piece::Bishop);
    let minors = knights.len() + bishops.len();
    if minors <= 1 {
        return true;
    }
    if !knights.is_empty() {
        return false;
    }
    // only bishops left: draw iff all on the same colour
    let mut light = 0;
    let mut dark = 0;
    for sq in bishops {
        if (sq.file() as usize + sq.rank() as usize) % 2 == 1 {
            light += 1;
        } else {
            dark += 1;
        }
    }
    // need at least one bishop on each side for the KB vs KB pattern; a lone
    // side with two same-coloured bishops cannot mate either
    light == 0 || dark == 0
}

/// Hash for repetition detection: the en-passant square only counts if an
/// en-passant capture is actually legal (FIDE definition).
pub fn repetition_key(board: &Board) -> u64 {
    if let Some(ep_file) = board.en_passant() {
        let stm = board.side_to_move();
        let ep_rank = cozy_chess::Rank::Sixth.relative_to(stm);
        let ep_sq = Square::new(ep_file, ep_rank);
        let mut ep_legal = false;
        board.generate_moves_for(board.colored_pieces(stm, Piece::Pawn), |mvs| {
            if mvs.to.has(ep_sq) {
                ep_legal = true;
                return true;
            }
            false
        });
        if ep_legal {
            return board.hash();
        }
    }
    board.hash_without_ep()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(fen: &str) -> Board {
        Board::from_fen(fen, false).unwrap()
    }

    #[test]
    fn san_cases() {
        let start = Board::default();
        let m = parse_uci(&start, "g1f3").unwrap();
        assert_eq!(san(&start, m), "Nf3");
        // castling + promotion with capture + check, from cozy-chess docs
        let bd = b("3k2n1/7P/Q3p3/4BPp1/Q1Q4q/8/5B2/R3K2R w KQ g6 0 1");
        assert_eq!(san(&bd, parse_uci(&bd, "h7g8r").unwrap()), "hxg8=R+");
        assert_eq!(san(&bd, parse_uci(&bd, "e1c1").unwrap()), "O-O-O+");
        assert_eq!(san(&bd, parse_uci(&bd, "e1g1").unwrap()), "O-O");
        assert_eq!(san(&bd, parse_uci(&bd, "e5d4").unwrap()), "Bd4");
        assert_eq!(san(&bd, parse_uci(&bd, "f5g6").unwrap()), "fxg6");
        // queens on a4, c4, a6: Qa4-b5 needs both file+rank? a4,c4 share rank 4; a4,a6 share file a
        assert_eq!(san(&bd, parse_uci(&bd, "a4b5").unwrap()), "Qa4b5");
        assert_eq!(san(&bd, parse_uci(&bd, "c4b5").unwrap()), "Qcb5");
        assert_eq!(san(&bd, parse_uci(&bd, "a6b5").unwrap()), "Q6b5");
        // mate
        let fm = b("rnbqkbnr/pppp1ppp/8/4p3/6P1/5P2/PPPPP2P/RNBQKBNR b KQkq g3 0 2");
        assert_eq!(san(&fm, parse_uci(&fm, "d8h4").unwrap()), "Qh4#");
        assert!(parse_uci(&start, "e2e5").is_none());
        assert!(parse_uci(&start, "0000").is_none());
        assert!(parse_uci(&start, "(none)").is_none());
    }

    #[test]
    fn uci_castling_must_be_standard() {
        // 1.e4 e5 2.Nf3 Nc6 3.Bc4 Nf6: White may castle short
        let mut b = Board::default();
        for m in ["e2e4", "e7e5", "g1f3", "b8c6", "f1c4", "g8f6"] {
            let mv = parse_uci(&b, m).unwrap();
            b.play_unchecked(mv);
        }
        assert!(parse_uci(&b, "e1g1").is_some());
        assert!(parse_uci(&b, "e1h1").is_none()); // king-takes-rook notation is not standard UCI
        assert!(nonstandard_castle_hint(&b, "e1h1").unwrap().contains("e1g1"));
        assert!(nonstandard_castle_hint(&b, "a1a1").is_none());
    }

    #[test]
    fn parse_san_cases() {
        let start = Board::default();
        assert_eq!(uci_string(&start, parse_san(&start, "Nf3!?").unwrap()), "g1f3");
        assert_eq!(uci_string(&start, parse_san(&start, "Ngf3").unwrap()), "g1f3");
        let bd = b("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        assert_eq!(uci_string(&bd, parse_san(&bd, "O-O").unwrap()), "e1g1");
        assert_eq!(uci_string(&bd, parse_san(&bd, "0-0-0+").unwrap()), "e1c1");
        let p = b("8/P6k/8/8/8/8/8/K7 w - - 0 1");
        assert_eq!(uci_string(&p, parse_san(&p, "a8=N").unwrap()), "a7a8n");
        assert_eq!(uci_string(&p, parse_san(&p, "a8Q").unwrap()), "a7a8q");
        assert!(parse_san(&start, "Nf4").is_none());
    }

    /// Cross-check our SAN writer against cozy-chess' on pseudo-random games.
    #[test]
    fn san_matches_cozy() {
        let mut seed = 0x1234_5678_9abc_def0u64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..300 {
            let mut bd = Board::default();
            for _ in 0..200 {
                let legal = legal_moves(&bd);
                if legal.is_empty() {
                    break;
                }
                for &m in &legal {
                    let ours = san(&bd, m);
                    let theirs = cozy_chess::util::display_san_move(&bd, m).to_string();
                    assert_eq!(ours, theirs, "fen {} move {:?}", bd, m);
                    assert_eq!(parse_san(&bd, &ours), Some(m));
                }
                let m = legal[(rnd() % legal.len() as u64) as usize];
                bd.play(m);
            }
        }
    }

    #[test]
    fn material_rules() {
        assert!(insufficient_material(&b("8/8/8/4k3/8/8/8/4K3 w - - 0 1")));
        assert!(insufficient_material(&b("8/8/8/4k3/8/8/8/4KN2 w - - 0 1")));
        assert!(insufficient_material(&b("8/8/8/4kb2/8/8/8/4KB2 w - - 0 1"))); // f5 light? f1 light
        assert!(!insufficient_material(&b("8/8/8/4k1b1/8/8/8/4KB2 w - - 0 1")));
        assert!(!insufficient_material(&b("8/8/8/4kn2/8/8/8/4KN2 w - - 0 1")));
        assert!(!insufficient_material(&b("8/8/8/4k3/8/8/4P3/4K3 w - - 0 1")));
        let bd = b("8/8/8/4k3/8/8/8/4KN2 w - - 0 1");
        assert!(cannot_mate(&bd, Color::White));
        assert!(cannot_mate(&bd, Color::Black));
        let bd = b("8/8/8/4k3/8/8/8/3NKN2 w - - 0 1");
        assert!(!cannot_mate(&bd, Color::White));
    }
}
