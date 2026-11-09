//! PGN reading (movetext tokenizer for opening suites) and writing (match games).

use crate::engine::{is_mate_score, MATE_SCORE};
use crate::game::{GameRecord, Tc};

#[derive(Debug, Default, Clone)]
pub struct PgnGame {
    pub tags: Vec<(String, String)>,
    /// SAN tokens (move numbers, comments, NAGs, variations and results removed).
    pub moves: Vec<String>,
    pub result: Option<String>,
}

impl PgnGame {
    #[allow(dead_code)] // used by tests / future subcommands
    pub fn tag(&self, name: &str) -> Option<&str> {
        self.tags.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
    fn is_empty(&self) -> bool {
        self.tags.is_empty() && self.moves.is_empty() && self.result.is_none()
    }
}

fn is_result(tok: &str) -> bool {
    matches!(tok, "1-0" | "0-1" | "1/2-1/2" | "*" | "½-½")
}

fn parse_tag(line: &str) -> Option<(String, String)> {
    let inner = line.trim().strip_prefix('[')?;
    let inner = inner.trim_end().strip_suffix(']').unwrap_or(inner);
    let (name, rest) = inner.trim().split_once(char::is_whitespace)?;
    let rest = rest.trim();
    let rest = rest.strip_prefix('"').unwrap_or(rest);
    let rest = rest.strip_suffix('"').unwrap_or(rest);
    let mut val = String::new();
    let mut esc = false;
    for c in rest.chars() {
        if esc {
            val.push(c);
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else {
            val.push(c);
        }
    }
    Some((name.to_string(), val))
}

/// Parse all games of a PGN text.
pub fn parse_pgn(text: &str) -> Vec<PgnGame> {
    let mut games = Vec::new();
    let mut cur = PgnGame::default();
    let mut tok = String::new();
    let mut in_brace = false;
    let mut var_depth = 0usize;

    fn flush_tok(tok: &mut String, cur: &mut PgnGame, games: &mut Vec<PgnGame>) {
        if tok.is_empty() {
            return;
        }
        let t = std::mem::take(tok);
        if is_result(&t) {
            cur.result = Some(if t == "½-½" { "1/2-1/2".to_string() } else { t });
            games.push(std::mem::take(cur));
            return;
        }
        if t.starts_with('$') {
            return; // NAG
        }
        // strip a move-number prefix: "12." "12..." "12...e5" "1.e4"
        let s = t.trim_start_matches(|c: char| c.is_ascii_digit());
        let s = if s.len() != t.len() && s.starts_with('.') {
            s.trim_start_matches('.')
        } else if s.len() != t.len() {
            // digits not followed by '.', e.g. a bare move number "12" — skip
            if s.is_empty() {
                return;
            }
            t.as_str()
        } else {
            s.trim_start_matches('.')
        };
        if s.is_empty() {
            return;
        }
        cur.moves.push(s.to_string());
    }

    for line in text.lines() {
        let trimmed = line.trim_start();
        if !in_brace && var_depth == 0 {
            if trimmed.starts_with('[') {
                flush_tok(&mut tok, &mut cur, &mut games);
                if !cur.moves.is_empty() || cur.result.is_some() {
                    games.push(std::mem::take(&mut cur));
                }
                if let Some(tag) = parse_tag(trimmed) {
                    cur.tags.push(tag);
                }
                continue;
            }
            if trimmed.starts_with('%') {
                continue; // escape line
            }
        }
        let chars = line.chars();
        for c in chars {
            if in_brace {
                if c == '}' {
                    in_brace = false;
                }
                continue;
            }
            match c {
                '{' => {
                    flush_tok(&mut tok, &mut cur, &mut games);
                    in_brace = true;
                }
                ';' => {
                    flush_tok(&mut tok, &mut cur, &mut games);
                    break; // rest-of-line comment
                }
                '(' => {
                    flush_tok(&mut tok, &mut cur, &mut games);
                    var_depth += 1;
                }
                ')' => {
                    var_depth = var_depth.saturating_sub(1);
                    tok.clear();
                }
                c if c.is_whitespace() => {
                    if var_depth == 0 {
                        flush_tok(&mut tok, &mut cur, &mut games);
                    } else {
                        tok.clear();
                    }
                }
                c => {
                    if var_depth == 0 {
                        tok.push(c);
                    }
                }
            }
        }
        if var_depth == 0 {
            flush_tok(&mut tok, &mut cur, &mut games);
        } else {
            tok.clear();
        }
    }
    flush_tok(&mut tok, &mut cur, &mut games);
    if !cur.is_empty() {
        games.push(cur);
    }
    games
}

// ---------------------------------------------------------------- writing

fn tag_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace(['\n', '\r'], " ")
}

fn comment_escape(s: &str) -> String {
    s.replace('}', ")").replace('{', "(").replace(['\n', '\r'], " ")
}

/// `+0.35` / `-1.20` / `+M3` / `-M2`
pub fn fmt_score(s: i32) -> String {
    if is_mate_score(s) {
        let n = MATE_SCORE - s.abs();
        format!("{}M{}", if s > 0 { "+" } else { "-" }, n)
    } else {
        format!("{:+.2}", s as f64 / 100.0)
    }
}

pub struct PgnMeta<'a> {
    pub event: &'a str,
    pub site: &'a str,
    pub tc: &'a Tc,
}

/// Render one game as PGN (ends with a blank line).
pub fn write_game(rec: &GameRecord, meta: &PgnMeta) -> String {
    let mut out = String::with_capacity(2048);
    let date = if rec.date.len() >= 10 { rec.date[..10].replace('-', ".") } else { "????.??.??".to_string() };
    let tc_tag = match meta.tc {
        Tc::Clock { .. } => meta.tc.label(),
        _ => "-".to_string(),
    };
    let mut tags: Vec<(&str, String)> = vec![
        ("Event", meta.event.to_string()),
        ("Site", meta.site.to_string()),
        ("Date", date),
        ("Round", rec.id.to_string()),
        ("White", rec.white.clone()),
        ("Black", rec.black.clone()),
        ("Result", rec.result.as_str().to_string()),
        ("Termination", rec.termination.as_str().to_string()),
        ("TimeControl", tc_tag),
    ];
    if !matches!(meta.tc, Tc::Clock { .. }) {
        tags.push(("ArenaTC", meta.tc.label()));
    }
    tags.push(("PlyCount", rec.moves.len().to_string()));
    tags.push(("OpeningIndex", rec.opening_index.to_string()));
    tags.push(("OpeningPlies", rec.opening_plies.to_string()));
    for (k, v) in &tags {
        out.push_str(&format!("[{} \"{}\"]\n", k, tag_escape(v)));
    }
    out.push('\n');

    // movetext tokens, wrapped at 79 columns
    let mut toks: Vec<String> = Vec::with_capacity(rec.moves.len() * 3 + 2);
    for i in 0..rec.san.len() {
        if i % 2 == 0 {
            toks.push(format!("{}.", i / 2 + 1));
        }
        toks.push(rec.san[i].clone());
        if i >= rec.opening_plies {
            let c = if rec.book.get(i).copied().unwrap_or(false) {
                "{book}".to_string()
            } else {
                let secs = format!("{:.3}s", rec.times_ms.get(i).copied().unwrap_or(0) as f64 / 1000.0);
                match (rec.scores.get(i).copied().flatten(), rec.depths.get(i).copied().flatten()) {
                    (Some(s), Some(d)) => format!("{{{}/{} {}}}", fmt_score(s), d, secs),
                    (Some(s), None) => format!("{{{} {}}}", fmt_score(s), secs),
                    (None, _) => format!("{{{}}}", secs),
                }
            };
            toks.push(c);
        }
    }
    if !rec.detail.is_empty() {
        toks.push(format!("{{{}}}", comment_escape(&rec.detail)));
    }
    toks.push(rec.result.as_str().to_string());

    let mut col = 0usize;
    for t in toks {
        // never break inside a comment: comments are single tokens here
        if col > 0 && col + 1 + t.len() > 79 {
            out.push('\n');
            col = 0;
        } else if col > 0 {
            out.push(' ');
            col += 1;
        }
        out.push_str(&t);
        col += t.chars().count();
    }
    out.push_str("\n\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let text = r#"[Event "?"]
[White "A \"x\""]
[Result "1/2-1/2"]

1. Nf3 d5 2. g3 {a comment
spanning lines} c6 (2... Nf6 3. Bg2 (3. d4) e6) 3. Bg2 $1 Nf6 ; rest of line
4. d3 Bg4 5. h3 Bh5 6. b3 e6 7. Bb2 Qa5+ 8.
Qd2 Qxd2+ 1/2-1/2

[Event "second"]

1.e4 e5 2.Nf3 Nc6 3...a6 *
"#;
        let g = parse_pgn(text);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].tag("White"), Some("A \"x\""));
        assert_eq!(
            g[0].moves,
            vec!["Nf3", "d5", "g3", "c6", "Bg2", "Nf6", "d3", "Bg4", "h3", "Bh5", "b3", "e6", "Bb2", "Qa5+", "Qd2", "Qxd2+"]
        );
        assert_eq!(g[0].result.as_deref(), Some("1/2-1/2"));
        assert_eq!(g[1].moves, vec!["e4", "e5", "Nf3", "Nc6", "a6"]);
        assert_eq!(g[1].result.as_deref(), Some("*"));
    }

    #[test]
    fn score_fmt() {
        assert_eq!(fmt_score(35), "+0.35");
        assert_eq!(fmt_score(-120), "-1.20");
        assert_eq!(fmt_score(0), "+0.00");
        assert_eq!(fmt_score(99997), "+M3");
        assert_eq!(fmt_score(-99998), "-M2");
    }
}
