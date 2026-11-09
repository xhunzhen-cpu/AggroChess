//! Pool of Stockfish evaluators (used by `analyze`).
//!
//! Evaluates a list of positions with a fixed node budget, `Threads=1`, a fixed
//! hash size and a fresh `ucinewgame` before every position, spreading the work
//! over N independent Stockfish processes (one worker thread each).
//!
//! Scores are returned exactly as Stockfish reports them: from the point of view
//! of the **side to move** in the evaluated position, in centipawns, mates encoded
//! like the rest of the crate (`±(100000 - n)`, see `engine::encode_mate`).
//! Callers are responsible for normalising to whichever side they care about.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::engine::{last_line, wait_bestmove, Engine, EngineConfig, SearchOutcome};
use crate::sys;

#[derive(Debug, Clone)]
pub struct SfSettings {
    pub path: String,
    pub nodes: u64,
    pub threads: usize,
    pub hash_mb: u32,
}

/// One position to evaluate. `position` is the argument of the UCI `position`
/// command, e.g. `startpos moves e2e4 e7e5` or `fen <fen>`.
#[derive(Debug, Clone)]
pub struct EvalJob {
    pub key: u64,
    pub position: String,
}

/// Outcome of a batch: scores by job key (side-to-move view, raw encoding);
/// keys whose evaluation failed twice are absent and listed in `failed`.
#[derive(Debug, Default)]
pub struct EvalBatch {
    pub scores: HashMap<u64, i32>,
    pub failed: Vec<u64>,
    pub restarts: usize,
    pub aborted: bool,
}

const UCIOK_TIMEOUT: Duration = Duration::from_secs(10);
const READY_TIMEOUT: Duration = Duration::from_secs(30);

impl SfSettings {
    fn engine_config(&self, idx: usize) -> EngineConfig {
        let mut cfg = EngineConfig::new(&format!("sf-eval-{}", idx), &self.path);
        cfg.options.push(("Threads".to_string(), "1".to_string()));
        cfg.options.push(("Hash".to_string(), self.hash_mb.to_string()));
        cfg
    }

    /// Generous per-position timeout: 30 s plus 1 s per 10k nodes.
    fn per_eval_timeout(&self) -> Duration {
        Duration::from_millis(30_000 + self.nodes / 10)
    }

    /// Quick start-up check (path, handshake) before launching the pool.
    pub fn check(&self) -> Result<String, String> {
        let mut e = Engine::start(&self.engine_config(0), UCIOK_TIMEOUT, READY_TIMEOUT)?;
        let id = e.id_name.clone().unwrap_or_else(|| "?".to_string());
        e.quit(Duration::from_millis(500));
        Ok(id)
    }
}

/// Evaluate one position on a running engine. Err = engine must be restarted.
fn eval_one(eng: &mut Engine, job: &EvalJob, s: &SfSettings) -> Result<Option<i32>, String> {
    eng.drain();
    eng.send("ucinewgame")?;
    eng.isready(READY_TIMEOUT)?;
    eng.send(&format!("position {}", job.position))?;
    eng.send(&format!("go nodes {}", s.nodes))?;
    match wait_bestmove(eng, Instant::now() + s.per_eval_timeout()) {
        SearchOutcome::Reply(r) => Ok(r.score),
        SearchOutcome::Timeout(l) => Err(format!("timeout ({})", last_line(&l))),
        SearchOutcome::Crashed(l) => Err(format!("engine died ({})", last_line(&l))),
        SearchOutcome::Aborted => Err("aborted".into()),
    }
}

/// Evaluate all jobs with `s.threads` Stockfish processes. Duplicate keys are
/// evaluated once. `label` prefixes the progress lines on stderr.
pub fn evaluate(s: &SfSettings, jobs: &[EvalJob], label: &str) -> EvalBatch {
    // dedup by key, keep first
    let mut seen = std::collections::HashSet::new();
    let uniq: Vec<EvalJob> = jobs.iter().filter(|j| seen.insert(j.key)).cloned().collect();
    let total = uniq.len();
    let mut batch = EvalBatch::default();
    if total == 0 {
        return batch;
    }
    let threads = s.threads.max(1).min(total);
    let jobs = Arc::new(uniq);
    let next = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(AtomicUsize::new(0));
    let results: Arc<Mutex<EvalBatch>> = Arc::new(Mutex::new(EvalBatch::default()));
    let t0 = Instant::now();
    errln!("{}: evaluating {} positions with {} Stockfish process(es), {} nodes each", label, total, threads, s.nodes);

    let mut handles = Vec::new();
    for w in 0..threads {
        let (jobs, next, done, results, s) = (jobs.clone(), next.clone(), done.clone(), results.clone(), s.clone());
        let h = thread::Builder::new().name(format!("sf-eval-{}", w)).spawn(move || {
            // The engine is spawned (and dies with) this thread: PR_SET_PDEATHSIG.
            let mut eng: Option<Engine> = None;
            loop {
                if sys::stop_requested() {
                    break;
                }
                let i = next.fetch_add(1, Ordering::SeqCst);
                if i >= jobs.len() {
                    break;
                }
                let job = &jobs[i];
                let mut score: Option<i32> = None;
                let mut ok = false;
                for attempt in 0..2 {
                    if eng.as_ref().map(|e| e.dead).unwrap_or(true) {
                        if let Some(mut e) = eng.take() {
                            e.kill();
                        }
                        match Engine::start(&s.engine_config(w), UCIOK_TIMEOUT, READY_TIMEOUT) {
                            Ok(e) => eng = Some(e),
                            Err(err) => {
                                errln!("sf-eval-{}: cannot start Stockfish: {}", w, err);
                                continue;
                            }
                        }
                    }
                    let e = eng.as_mut().unwrap();
                    match eval_one(e, job, &s) {
                        Ok(sc) => {
                            score = sc;
                            ok = sc.is_some();
                            if !ok {
                                errln!("sf-eval-{}: no score for position {} (attempt {})", w, job.key, attempt + 1);
                            }
                        }
                        Err(err) => {
                            if !sys::stop_requested() {
                                errln!("sf-eval-{}: {} on position {} (attempt {})", w, err, job.key, attempt + 1);
                            }
                            e.kill();
                            results.lock().unwrap().restarts += 1;
                        }
                    }
                    if ok || sys::stop_requested() {
                        break;
                    }
                }
                {
                    let mut r = results.lock().unwrap();
                    match (ok, score) {
                        (true, Some(sc)) => {
                            r.scores.insert(job.key, sc);
                        }
                        _ => r.failed.push(job.key),
                    }
                }
                done.fetch_add(1, Ordering::SeqCst);
            }
            if let Some(mut e) = eng.take() {
                e.quit(Duration::from_millis(500));
            }
        });
        match h {
            Ok(h) => handles.push(h),
            Err(e) => errln!("cannot spawn evaluator thread: {}", e),
        }
    }

    // progress
    let mut last_print = Instant::now();
    let mut last_done = 0;
    while handles.iter().any(|h| !h.is_finished()) {
        thread::sleep(Duration::from_millis(200));
        let d = done.load(Ordering::SeqCst);
        if d != last_done && last_print.elapsed() >= Duration::from_secs(10) {
            let el = t0.elapsed().as_secs_f64();
            let rate = d as f64 / el.max(1e-9);
            let eta = if rate > 0.0 { (total - d) as f64 / rate } else { 0.0 };
            errln!("{}: {}/{} positions ({:.1}/s, ~{:.0}s left)", label, d, total, rate, eta);
            last_print = Instant::now();
            last_done = d;
        }
    }
    for h in handles {
        let _ = h.join();
    }
    let mut r = std::mem::take(&mut *results.lock().unwrap());
    r.aborted = sys::stop_requested();
    batch.scores = std::mem::take(&mut r.scores);
    batch.failed = std::mem::take(&mut r.failed);
    batch.restarts = r.restarts;
    batch.aborted = r.aborted;
    errln!(
        "{}: done, {} evaluated, {} failed, {:.1}s",
        label,
        batch.scores.len(),
        batch.failed.len(),
        t0.elapsed().as_secs_f64()
    );
    batch
}
