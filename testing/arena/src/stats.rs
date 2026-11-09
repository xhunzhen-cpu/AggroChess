//! W/D/L statistics: score, Elo difference with a 95% CI from the trinomial
//! normal approximation, likelihood of superiority.

#[derive(Debug, Clone, Copy)]
pub struct WdlStats {
    pub score: f64,
    pub draw_ratio: f64,
    /// Elo difference (None for 0% / 100% scores or no games).
    pub elo: Option<f64>,
    /// 95% CI bounds (None if the bound falls outside (0,1) score, i.e. unbounded).
    /// Trinomial normal approximation; Wilson score interval when every game had the
    /// same result (zero variance).
    pub elo_lo: Option<f64>,
    pub elo_hi: Option<f64>,
    /// Standard error of the mean score per game.
    pub score_se: f64,
    /// Likelihood of superiority (from decisive games).
    pub los: Option<f64>,
}

pub fn elo_from_score(s: f64) -> Option<f64> {
    if s <= 0.0 || s >= 1.0 || !s.is_finite() {
        None
    } else {
        Some(-400.0 * (1.0 / s - 1.0).log10())
    }
}

/// Wilson 95% score interval for a fraction `p` observed over `n` trials.
pub fn wilson(p: f64, n: f64) -> (f64, f64) {
    let z = 1.959964f64;
    let z2 = z * z;
    let den = 1.0 + z2 / n;
    let centre = (p + z2 / (2.0 * n)) / den;
    let half = z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt() / den;
    let (lo, hi) = (centre - half, centre + half);
    // p = 0 / p = 1 give exactly 0 / 1 mathematically; snap the rounding error
    (if lo < 1e-9 { 0.0 } else { lo }, if hi > 1.0 - 1e-9 { 1.0 } else { hi })
}

pub fn wdl_stats(w: u64, d: u64, l: u64) -> WdlStats {
    let n = w + d + l;
    if n == 0 {
        return WdlStats {
            score: 0.0,
            draw_ratio: 0.0,
            elo: None,
            elo_lo: None,
            elo_hi: None,
            score_se: 0.0,
            los: None,
        };
    }
    let nf = n as f64;
    let (wf, df, lf) = (w as f64 / nf, d as f64 / nf, l as f64 / nf);
    let s = wf + 0.5 * df;
    let var = wf * (1.0 - s).powi(2) + df * (0.5 - s).powi(2) + lf * (0.0 - s).powi(2);
    let se = (var / nf).sqrt();
    // Every game had the same result (all wins / all draws / all losses): the trinomial
    // variance is 0 and s +- 1.96 SE would collapse to a point (or claim an unbounded
    // upper end for 0-N). Use the Wilson score interval on score points instead.
    let (lo, hi) = if var <= 1e-12 { wilson(s, nf) } else { (s - 1.959964 * se, s + 1.959964 * se) };
    let los = if w + l > 0 {
        Some(0.5 * (1.0 + erf((w as f64 - l as f64) / (2.0 * (w + l) as f64).sqrt())))
    } else {
        None
    };
    WdlStats {
        score: s,
        draw_ratio: df,
        elo: elo_from_score(s),
        elo_lo: elo_from_score(lo),
        elo_hi: elo_from_score(hi),
        score_se: se,
        los,
    }
}

/// erf with |error| < 1.2e-7 (Numerical Recipes erfc approximation).
pub fn erf(x: f64) -> f64 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let r = t
        * (-z * z - 1.26551223
            + t * (1.00002368
                + t * (0.37409196
                    + t * (0.09678418
                        + t * (-0.18628806
                            + t * (0.27886807
                                + t * (-1.13520398 + t * (1.48851587 + t * (-0.82215223 + t * 0.17087277)))))))))
            .exp();
    if x >= 0.0 {
        1.0 - r
    } else {
        r - 1.0
    }
}

pub fn fmt_elo(v: Option<f64>) -> String {
    match v {
        Some(x) => {
            let x = if x.abs() < 0.05 { 0.0 } else { x }; // avoid "-0.0"
            format!("{:+.1}", x)
        }
        None => "n/a".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basics() {
        assert!((elo_from_score(0.75).unwrap() - 190.85).abs() < 0.01);
        assert!((erf(1.0) - 0.8427007929).abs() < 1e-6);
        let st = wdl_stats(10, 10, 10);
        assert!((st.score - 0.5).abs() < 1e-12);
        assert!(st.elo.unwrap().abs() < 1e-9);
        assert!((st.los.unwrap() - 0.5).abs() < 1e-6);
        let st = wdl_stats(5, 0, 0);
        assert!(st.elo.is_none());
        // zero variance: Wilson interval instead of a point / a fake unbounded end
        let st = wdl_stats(0, 0, 10);
        assert!(st.elo.is_none() && st.elo_lo.is_none());
        assert!((st.elo_hi.unwrap() - -166.0).abs() < 2.0, "{:?}", st.elo_hi);
        let st = wdl_stats(10, 0, 0);
        assert!(st.elo_hi.is_none() && (st.elo_lo.unwrap() - 166.0).abs() < 2.0, "{:?} {:?}", st.elo_lo, st.elo_hi);
        let st = wdl_stats(0, 10, 0);
        let (lo, hi) = (st.elo_lo.unwrap(), st.elo_hi.unwrap());
        assert!((lo + hi).abs() < 1e-6 && hi > 150.0, "{} {}", lo, hi);
    }
}
