//! `openings` subcommand (PGN → UCI move lines) and the opening-file loader.

use std::io::Write;
use std::path::Path;

use cozy_chess::Board;

use crate::pgn;
use crate::san;

pub struct Opening {
    /// 0-based line index in the file.
    pub index: i64,
    pub moves: Vec<String>,
}

/// Load an openings file (one line of space-separated UCI moves per opening;
/// blank lines and lines starting with '#' are skipped). Every line is replayed
/// and checked for legality; invalid lines are reported and skipped.
pub fn load_openings(path: &Path) -> Result<Vec<Opening>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let mut out = Vec::new();
    let mut bad = 0usize;
    for (i, line) in text.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let mut board = Board::default();
        let mut moves = Vec::new();
        let mut ok = true;
        for u in t.split_whitespace() {
            match san::parse_uci(&board, u) {
                Some(m) => {
                    moves.push(san::uci_string(&board, m));
                    board.play_unchecked(m);
                }
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            out.push(Opening { index: i as i64, moves });
        } else {
            bad += 1;
            if bad <= 5 {
                errln!("warning: {}:{}: illegal/unparsable opening skipped", path.display(), i + 1);
            }
        }
    }
    if bad > 5 {
        errln!("warning: {} invalid openings skipped in total", bad);
    }
    if out.is_empty() {
        return Err(format!("{}: no valid openings", path.display()));
    }
    Ok(out)
}

/// Replay SAN tokens from the start position; Err(ply, token) on failure.
pub fn san_to_uci(tokens: &[String]) -> Result<Vec<String>, (usize, String)> {
    let mut board = Board::default();
    let mut out = Vec::with_capacity(tokens.len());
    for (i, t) in tokens.iter().enumerate() {
        match san::parse_san(&board, t) {
            Some(m) => {
                out.push(san::uci_string(&board, m));
                board.play_unchecked(m);
            }
            None => return Err((i, t.clone())),
        }
    }
    Ok(out)
}

pub fn cmd(args: &[String]) -> i32 {
    let mut pos: Vec<&String> = Vec::new();
    let mut plies: Option<usize> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--plies" => {
                plies = args.get(i + 1).and_then(|v| v.parse().ok());
                if plies.is_none() {
                    errln!("--plies needs a number");
                    return 2;
                }
                i += 2;
            }
            "-h" | "--help" => {
                outln!("usage: aggro_arena openings <in.pgn> <out.txt> [--plies N]");
                return 0;
            }
            _ => {
                pos.push(&args[i]);
                i += 1;
            }
        }
    }
    if pos.len() != 2 {
        errln!("usage: aggro_arena openings <in.pgn> <out.txt> [--plies N]");
        return 2;
    }
    let bytes = match std::fs::read(pos[0]) {
        Ok(b) => b,
        Err(e) => {
            errln!("{}: {}", pos[0], e);
            return 1;
        }
    };
    let text = String::from_utf8_lossy(&bytes);
    let games = pgn::parse_pgn(&text);
    let mut lines: Vec<String> = Vec::with_capacity(games.len());
    let mut skipped = 0usize;
    let mut empty = 0usize;
    let mut ply_hist: std::collections::BTreeMap<usize, usize> = Default::default();
    for (gi, g) in games.iter().enumerate() {
        if g.moves.is_empty() {
            empty += 1;
            continue;
        }
        let toks: &[String] = match plies {
            Some(n) if n < g.moves.len() => &g.moves[..n],
            _ => &g.moves[..],
        };
        match san_to_uci(toks) {
            Ok(u) => {
                *ply_hist.entry(u.len()).or_default() += 1;
                lines.push(u.join(" "));
            }
            Err((ply, tok)) => {
                skipped += 1;
                if skipped <= 10 {
                    errln!("skip game #{}: cannot parse '{}' at ply {}", gi + 1, tok, ply + 1);
                }
            }
        }
    }
    let mut f = match std::fs::File::create(pos[1]) {
        Ok(f) => std::io::BufWriter::new(f),
        Err(e) => {
            errln!("{}: {}", pos[1], e);
            return 1;
        }
    };
    for l in &lines {
        if writeln!(f, "{}", l).is_err() {
            errln!("write error");
            return 1;
        }
    }
    if f.flush().is_err() {
        errln!("write error");
        return 1;
    }
    outln!(
        "games read: {}, openings written: {}, skipped (parse errors): {}, skipped (no moves): {}",
        games.len(),
        lines.len(),
        skipped,
        empty
    );
    let hist: Vec<String> = ply_hist.iter().map(|(k, v)| format!("{}:{}", k, v)).collect();
    outln!("plies per opening (plies:count): {}", hist.join(" "));
    0
}
