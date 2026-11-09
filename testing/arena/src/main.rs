//! aggro_arena — self-contained testing harness for AggroChess.
//!
//! Subcommands (each lives in its own module; add new ones to `main`):
//!   openings  PGN opening suite -> UCI move lines
//!   match     engine-vs-engine tournament (PGN + JSONL + summary JSON)
//!   ucitest   scripted UCI conformance test of one engine
//!   summary   recompute W/D/L + Elo from existing JSONL files
//!   analyze   sacrifice detection / classification on JSONL output (+ Stockfish)

/// `println!` replacement that never panics on a closed stdout (see `sys::out_str`).
macro_rules! outln {
    () => { $crate::sys::out_str("\n") };
    ($($arg:tt)*) => {{
        let mut s = format!($($arg)*);
        s.push('\n');
        $crate::sys::out_str(&s)
    }};
}

/// `print!` replacement that never panics on a closed stdout.
macro_rules! out {
    ($($arg:tt)*) => { $crate::sys::out_str(&format!($($arg)*)) };
}

/// `eprintln!` replacement that never panics on a closed stderr.
macro_rules! errln {
    ($($arg:tt)*) => {{
        let mut s = format!($($arg)*);
        s.push('\n');
        $crate::sys::err_str(&s)
    }};
}

mod analyze;
mod engine;
mod game;
mod json;
mod openings;
mod pgn;
mod san;
mod sfeval;
mod stats;
mod sys;
mod tournament;
mod ucitest;

const USAGE: &str = "\
aggro_arena <subcommand> [args]

  openings <in.pgn> <out.txt> [--plies N]
      Convert PGN (SAN) games into UCI move lines from the start position.
  match <config>
      Run a tournament described by a config file (see testing/README.md).
  ucitest <engine-path> [--option \"Name=Value\"]... [--cwd DIR] [--arg ARG]...
      Scripted UCI conformance test; exits 1 if any check fails.
  summary <games.jsonl>...
      Recompute W/D/L and Elo per engine pairing from match JSONL output.
  analyze <games.jsonl>... --engine <name>[,<name>...] [--sf <stockfish>] [--sf-nodes N]
          [--threads N] [--sf-hash MB] [--out <prefix>] [--quiet]
      Detect and classify the engine's sacrifices (queen/exchange/piece/pawn), filter
      blunders, score outcomes; with --sf also soundness via Stockfish. Writes
      <prefix>.json and <prefix>.events.jsonl.
";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rest = if args.len() > 2 { &args[2..] } else { &[][..] };
    let code = match args.get(1).map(|s| s.as_str()) {
        Some("openings") => openings::cmd(rest),
        Some("match") => tournament::cmd(rest),
        Some("ucitest") => ucitest::cmd(rest),
        Some("summary") => tournament::summary_cmd(rest),
        Some("analyze") => analyze::cmd(rest),
        Some("-h") | Some("--help") | Some("help") => {
            print!("{}", USAGE);
            0
        }
        Some(other) => {
            eprintln!("unknown subcommand '{}'\n", other);
            eprint!("{}", USAGE);
            2
        }
        None => {
            eprint!("{}", USAGE);
            2
        }
    };
    std::process::exit(code);
}
