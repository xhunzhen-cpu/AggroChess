//! The five personality controls of the Ultimate Sacrificial Edition, exposed as UCI options,
//! and how each one maps onto the engine's evaluation and search parameters.
//!
//! Every control runs from 0 to 100. The *classic* values (Aggression 60, Sacrifice Appetite 75,
//! King Attack Priority 60, Material Preservation 50, Speculative Sacrifices 50) reproduce the
//! fixed settings of AggroChess 4.5.0 exactly. The defaults (90 / 95 / 90 / 15 / 90) go well
//! beyond them.
//!
//! The personality is independent of the Skill Level: nothing here reads the Skill Level, and
//! the Skill Level code (`skill.rs`) never writes any of these values.
//!
//! What each control drives:
//!
//! * **Aggression**: how much the engine wants a sharp, complicated game: trade aversion (keep
//!   pieces and especially queens on), contempt (a draw counts as a small loss), a bonus for
//!   active, protected pieces in the enemy half and for pieces that attack enemy pieces
//!   (initiative), and the preference for forcing moves among near-equal ones.
//! * **King Attack Priority**: how much the attack on the enemy king is worth: the extra king
//!   danger the engine creates, pieces aimed at the king zone, rooks and queens on open files
//!   next to the king, the "king clamp" pawn on the 6th rank, and how early king-zone moves are
//!   tried by the search.
//! * **Sacrifice Appetite**: how much the engine *wants* to sacrifice: the size of the bonus for a
//!   sacrifice aimed at the king or an exchange sacrifice, how much of that appetite remains once
//!   it has already sacrificed in a game, chained (Tal) sacrifices, how early in the game its
//!   hunger for a first sacrifice starts to grow, and a smaller bonus for gambits (pawn
//!   sacrifices for the attack).
//! * **Speculative Sacrifices**: how *unsound* a sacrifice may be: how far below the objective
//!   score a sacrificial line may fall and still earn the bonus, how much attack has to stand
//!   behind it, whether leaving material hanging counts, and how leniently the search treats
//!   sacrificial moves (they are pruned later and extended more).
//! * **Material Preservation**: how much a material deficit hurts in the evaluation. Below 50, a
//!   deficit is partly discounted while the engine has attackers around the enemy king (it keeps
//!   attacking instead of rushing to win the material back); above 50, a deficit costs extra.

use crate::{eval, search};
use std::sync::atomic::{AtomicI32, Ordering};

/// Default values of the five controls (UCI defaults).
pub const DEFAULTS: Settings = Settings {
    aggression: 90,
    sacrifice_appetite: 95,
    king_attack: 90,
    material_preservation: 15,
    speculative: 90,
};

/// The values that reproduce AggroChess 4.5.0.
pub const CLASSIC: Settings = Settings {
    aggression: 60,
    sacrifice_appetite: 75,
    king_attack: 60,
    material_preservation: 50,
    speculative: 50,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub aggression: i32,
    pub sacrifice_appetite: i32,
    pub king_attack: i32,
    pub material_preservation: i32,
    pub speculative: i32,
}

pub static AGGRESSION: AtomicI32 = AtomicI32::new(DEFAULTS.aggression);
pub static SACRIFICE_APPETITE: AtomicI32 = AtomicI32::new(DEFAULTS.sacrifice_appetite);
pub static KING_ATTACK: AtomicI32 = AtomicI32::new(DEFAULTS.king_attack);
pub static MATERIAL_PRESERVATION: AtomicI32 = AtomicI32::new(DEFAULTS.material_preservation);
pub static SPECULATIVE: AtomicI32 = AtomicI32::new(DEFAULTS.speculative);

/// The internal parameters derived from the five controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Derived {
    // Aggression
    pub trade_k: i32,
    pub activity_k: i32,
    pub contempt: i32,
    pub forcing_tiebreak: i32,
    // King Attack Priority
    pub king_danger_pct: i32,
    pub king_k: i32,
    pub zone_order: i32,
    // Sacrifice Appetite
    pub sac_bonus: i32,
    pub after_sac_bonus: i32,
    pub chain_bonus: i32,
    pub pressure_start: i32,
    pub pressure_step: i32,
    pub gambit_bonus: i32,
    // Speculative Sacrifices
    pub sac_floor: i32,
    pub sac_full_units: i32,
    pub sac_partial_hi: i32,
    pub sac_partial_lo: i32,
    pub tal_sacs: bool,
    pub check_see: i32,
    pub sac_ext_depth: i32,
    // Material Preservation
    pub material_preservation: i32,
}

/// Map the five controls (each clamped to 0..=100) to the engine's internal parameters.
pub fn derive(s: Settings) -> Derived {
    let a = s.aggression.clamp(0, 100);
    let sa = s.sacrifice_appetite.clamp(0, 100);
    let k = s.king_attack.clamp(0, 100);
    let m = s.material_preservation.clamp(0, 100);
    let p = s.speculative.clamp(0, 100);
    // Sacrifice Appetite. The bonus is how far below the best alternative a sacrifice may score
    // and still be played, so it is the engine's tolerance for unsound sacrifices. Up to the
    // classic 75 the mapping is 4.5.0's. Above 75 the appetite is made *persistent* rather than
    // *huge*: the bonus for the game's first sacrifice is held small (20 at 95) while the
    // appetite after it, which 4.5.0 drops to 60 (nearly sound sacrifices only), stays at 180, so
    // a speculative sacrifice of a pawn or two is welcome all game. The size of that first bonus
    // is what decides how far below sound a sacrifice may be, and it was measured to be far too
    // large at 340: on the same openings and 600-game draws a first-sacrifice bonus of 20 raises
    // the strict chess.com brilliant rate from 0.065 to 0.113 per game AND is ~50 Elo *stronger*
    // head-to-head (0/5/10/20/40/60/120/220/340 give 0.108/0.067/0.080/0.113/0.092/0.087/0.093/
    // 0.078/0.065, peaking at 20). The bonus is what buys junk *inside* the badge-shaped window:
    // on the same 600 games (shipped 340 as ctl, base 20 as var) the badge-shaped events inside
    // moves 10-30 stay level (0.235 -> 0.250 per game), the unsound share of them falls
    // (95 -> 69) and the conversion inside the window rises (18.4% -> 31.3%) - so total
    // sacrifice volume is roughly flat while the strict count rises by half
    // (on the same 600 games base 20 fires about the same number of sacrifices as base 340 -
    // 835 vs 819 - but half again as many strict brilliants, 68 vs 45).
    // The pressure ramp added on top of this bonus is *load-bearing* once the base is small.
    // At base 340 it bought no brilliants at all (neutralising it left strict exactly +0.000/g)
    // while COSTING 35 Elo, so it only ever hurt; at base 20 it buys +0.042 strict/g for about
    // 12 Elo. The winning shape is therefore *small early* (no junk in the badge window)
    // *growing late* (the ramp supplies the +450 late-game bonus). Its brilliants come from the
    // floor half, not the bonus half: killing only the floor drop (x_timefloor=0) drops strict
    // 0.113 -> 0.073, while killing only the bonus (x_pstep=0) leaves 0.072 - the bonus half is
    // worth ~0. See also `PRESSURE_FLOOR_NUM`.
    // The peak at 20 is a single-draw feature: on other seeds base 0 ties base 20 on strict
    // (paired +0.0006/g over 3 seeds), and base 0 is in fact the *stronger* of the two — on a
    // direct same-binary head-to-head (only this knob differing) base 0 wins 301-220, i.e. ~47
    // Elo, and it is 59-84 Elo stronger than base 20 against the shipped 340 on three seeds.
    // The reason to keep the base above 0 is not strength: a 0 bonus trips the early return in
    // sac_params_for and throws away the whole search-level sacrifice subsystem (ramp, floor
    // drag, chain, after-sac, gambit, forcing tie-break), which this default wants to keep.
    // (An earlier version kept 240 of a 380 bonus and started the pressure at move 12; measured,
    // it gave fewer purposeful sacrifices per move in balanced positions than 4.5.0, about twice
    // the blunders, and lost about 170 Elo: tolerating four-pawn howlers is not the same as
    // sacrificing more.)
    let sac_bonus = if sa <= 75 { 4 * sa } else { 20 };
    let after_sac_bonus = if sa <= 75 { 60 * sa / 75 } else { 60 + 6 * (sa - 75) };
    let chain_bonus = if sa <= 75 { 2 * sa } else { 150 + 2 * (sa - 75) };
    let pressure_start = if sa <= 75 { 20 + (75 - sa) * 2 / 5 } else { 20 - (sa - 75) * 3 / 10 };
    let pressure_step = if sa <= 75 { sa / 5 } else { 15 };
    let gambit_bonus = 2 * (sa - 75).max(0);
    // Speculative Sacrifices: the floor below which a sacrificial line earns no bonus at all.
    let sac_floor = if p <= 50 { -(100 + p * 21 / 5) } else { -310 - 3 * (p - 50) };
    // King Attack Priority: up to the classic 60 as in 4.5.0 (2 points per step); above it at
    // half that slope. Measured: the full slope (280% king danger at 90) cost about 140 Elo
    // head-to-head on its own, because the inflated attack scores mislead the search.
    let king_danger_pct = if k <= 60 { 100 + 2 * k } else { 220 + (k - 60) };
    let king_k = if k <= 60 { 2 * k } else { 120 + (k - 60) };
    let zone_order = if k <= 60 { 2500 * k / 60 } else { 2500 + 25 * (k - 60) };
    Derived {
        trade_k: 2 * a,
        activity_k: (2 * a - 120).max(0),
        contempt: a * 5 / 6,
        forcing_tiebreak: a / 5,
        king_danger_pct,
        king_k,
        zone_order,
        sac_bonus,
        after_sac_bonus,
        chain_bonus,
        pressure_start,
        pressure_step,
        gambit_bonus,
        sac_floor,
        sac_full_units: if p >= 75 { 2 } else { 3 },
        sac_partial_hi: 60 + (p - 50).max(0) * 2 / 5,
        sac_partial_lo: 25 + (p - 50).max(0) / 4,
        tal_sacs: p >= 70,
        check_see: 200 + 3 * (p - 50).max(0),
        sac_ext_depth: if p >= 75 { 3 } else { 4 },
        material_preservation: m,
    }
}

/// The current settings.
pub fn current() -> Settings {
    Settings {
        aggression: AGGRESSION.load(Ordering::Relaxed),
        sacrifice_appetite: SACRIFICE_APPETITE.load(Ordering::Relaxed),
        king_attack: KING_ATTACK.load(Ordering::Relaxed),
        material_preservation: MATERIAL_PRESERVATION.load(Ordering::Relaxed),
        speculative: SPECULATIVE.load(Ordering::Relaxed),
    }
}

/// Store new settings and push the derived parameters into the evaluation and the search.
pub fn set(s: Settings) {
    AGGRESSION.store(s.aggression.clamp(0, 100), Ordering::Relaxed);
    SACRIFICE_APPETITE.store(s.sacrifice_appetite.clamp(0, 100), Ordering::Relaxed);
    KING_ATTACK.store(s.king_attack.clamp(0, 100), Ordering::Relaxed);
    MATERIAL_PRESERVATION.store(s.material_preservation.clamp(0, 100), Ordering::Relaxed);
    SPECULATIVE.store(s.speculative.clamp(0, 100), Ordering::Relaxed);
    apply();
}

/// Push the derived parameters of the current settings into the evaluation and the search.
pub fn apply() {
    let d = derive(current());
    let r = Ordering::Relaxed;
    // any settings change invalidates every cached personality value
    eval::PERS_VERSION.fetch_add(1, r);
    eval::TRADE_K.store(d.trade_k, r);
    eval::ACTIVITY_K.store(d.activity_k, r);
    search::CONTEMPT.store(d.contempt, r);
    search::FORCING_TIEBREAK.store(d.forcing_tiebreak, r);
    eval::AGGRESSION.store(d.king_danger_pct, r);
    eval::KING_K.store(d.king_k, r);
    search::ZONE_ORDER.store(d.zone_order, r);
    search::SACRIFICE_BONUS.store(d.sac_bonus, r);
    // the Tal look follows the appetite SETTING, not the derived bonus: see APPETITE_ON
    search::APPETITE_ON.store(current().sacrifice_appetite > 0, r);
    search::AFTER_SAC_BONUS.store(d.after_sac_bonus, r);
    search::CHAIN_BONUS.store(d.chain_bonus, r);
    search::PRESSURE_START.store(d.pressure_start, r);
    search::PRESSURE_STEP.store(d.pressure_step, r);
    search::GAMBIT_BONUS.store(d.gambit_bonus, r);
    search::SAC_FLOOR.store(d.sac_floor, r);
    search::SAC_FULL_UNITS.store(d.sac_full_units, r);
    search::SAC_PARTIAL_HI.store(d.sac_partial_hi, r);
    search::SAC_PARTIAL_LO.store(d.sac_partial_lo, r);
    search::TAL_SACS.store(d.tal_sacs, r);
    search::CHECK_SEE.store(d.check_see, r);
    search::SAC_EXT_DEPTH.store(d.sac_ext_depth, r);
    eval::MATERIAL_PRES.store(d.material_preservation, r);
}

/// Neutral settings for self-play data generation: the search parameters of AggroChess 4.5.0
/// with every personality weight at neutral (no sacrifice bonus, no contempt, a symmetric
/// evaluation), so the training labels are the engine's objective judgement. The datagen
/// command also clears the engine colour, which switches the evaluation's personality off.
pub fn set_datagen_neutral() {
    set(CLASSIC);
    let r = Ordering::Relaxed;
    eval::AGGRESSION.store(100, r);
    eval::TRADE_K.store(0, r);
    eval::KING_K.store(0, r);
    eval::ACTIVITY_K.store(0, r);
    eval::MATERIAL_PRES.store(50, r);
    search::SACRIFICE_BONUS.store(0, r);
    search::GAMBIT_BONUS.store(0, r);
    search::CONTEMPT.store(0, r);
}

/// The derived parameters as they currently stand in the engine (they can differ from
/// `derive(current())` after a developer knob changed one of them directly).
pub fn live() -> Derived {
    let r = Ordering::Relaxed;
    Derived {
        trade_k: eval::TRADE_K.load(r),
        activity_k: eval::ACTIVITY_K.load(r),
        contempt: search::CONTEMPT.load(r),
        forcing_tiebreak: search::FORCING_TIEBREAK.load(r),
        king_danger_pct: eval::AGGRESSION.load(r),
        king_k: eval::KING_K.load(r),
        zone_order: search::ZONE_ORDER.load(r),
        sac_bonus: search::SACRIFICE_BONUS.load(r),
        after_sac_bonus: search::AFTER_SAC_BONUS.load(r),
        chain_bonus: search::CHAIN_BONUS.load(r),
        pressure_start: search::PRESSURE_START.load(r),
        pressure_step: search::PRESSURE_STEP.load(r),
        gambit_bonus: search::GAMBIT_BONUS.load(r),
        sac_floor: search::SAC_FLOOR.load(r),
        sac_full_units: search::SAC_FULL_UNITS.load(r),
        sac_partial_hi: search::SAC_PARTIAL_HI.load(r),
        sac_partial_lo: search::SAC_PARTIAL_LO.load(r),
        tal_sacs: search::TAL_SACS.load(r),
        check_see: search::CHECK_SEE.load(r),
        sac_ext_depth: search::SAC_EXT_DEPTH.load(r),
        material_preservation: eval::MATERIAL_PRES.load(r),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The classic settings must reproduce the fixed constants of AggroChess 4.5.0.
    #[test]
    fn classic_reproduces_4_5_0() {
        let _lock = crate::test_lock();
        let d = derive(CLASSIC);
        assert_eq!(d.trade_k, 120); // 4.5.0: k = AGGRESSION - 100 = 120
        assert_eq!(d.activity_k, 0);
        assert_eq!(d.contempt, 50);
        assert_eq!(d.forcing_tiebreak, 12);
        assert_eq!(d.king_danger_pct, 220);
        assert_eq!(d.king_k, 120);
        assert_eq!(d.zone_order, 2500);
        assert_eq!(d.sac_bonus, 300);
        assert_eq!(d.after_sac_bonus, 60);
        assert_eq!(d.chain_bonus, 150);
        assert_eq!(d.pressure_start, 20);
        assert_eq!(d.pressure_step, 15);
        assert_eq!(d.gambit_bonus, 0);
        assert_eq!(d.sac_floor, -310);
        assert_eq!(d.sac_full_units, 3);
        assert_eq!(d.sac_partial_hi, 60);
        assert_eq!(d.sac_partial_lo, 25);
        assert!(!d.tal_sacs);
        assert_eq!(d.check_see, 200);
        assert_eq!(d.sac_ext_depth, 4);
        assert_eq!(d.material_preservation, 50);
    }

    /// The engine starts with the defaults even before any `setoption` (the statics' initial
    /// values are the defaults' derived parameters).
    #[test]
    fn statics_start_at_the_defaults() {
        let _lock = crate::test_lock();
        // other tests may change the live values; compare against the statics' declared
        // initial values via a fresh derive of the defaults and the documented numbers
        let d = derive(DEFAULTS);
        assert_eq!(d, INITIAL);
    }

    /// Each control moves its parameters monotonically in the more aggressive direction.
    #[test]
    fn controls_are_monotone() {
        let _lock = crate::test_lock();
        let mut prev = derive(Settings { aggression: 0, sacrifice_appetite: 0, king_attack: 0, material_preservation: 100, speculative: 0 });
        for v in 1..=100 {
            let d = derive(Settings { aggression: v, sacrifice_appetite: v, king_attack: v, material_preservation: 100 - v, speculative: v });
            assert!(d.trade_k >= prev.trade_k && d.activity_k >= prev.activity_k && d.contempt >= prev.contempt);
            assert!(d.king_danger_pct >= prev.king_danger_pct && d.king_k >= prev.king_k && d.zone_order >= prev.zone_order);
            // sac_bonus is deliberately NOT monotone in the appetite. Above the classic 75 the
            // appetite is made *persistent* (after_sac_bonus, chain_bonus keep rising) rather than
            // *huge*, and the first-sacrifice bonus is held small: measured, a smaller first bonus
            // raises the strict brilliant rate and is stronger head-to-head (see derive()).
            assert!(d.after_sac_bonus >= prev.after_sac_bonus && d.chain_bonus >= prev.chain_bonus);
            assert!(d.pressure_start <= prev.pressure_start && d.pressure_step >= prev.pressure_step && d.gambit_bonus >= prev.gambit_bonus);
            assert!(d.sac_floor <= prev.sac_floor && d.sac_full_units <= prev.sac_full_units);
            assert!(d.sac_partial_hi >= prev.sac_partial_hi && d.sac_partial_lo >= prev.sac_partial_lo);
            assert!(d.check_see >= prev.check_see && d.sac_ext_depth <= prev.sac_ext_depth);
            assert!(d.material_preservation <= prev.material_preservation);
            prev = d;
        }
    }
}

/// The derived parameters of `DEFAULTS`, written out: the initial values of the engine's
/// statics (checked by a test against `derive(DEFAULTS)`).
pub const INITIAL: Derived = Derived {
    trade_k: 180,
    activity_k: 60,
    contempt: 75,
    forcing_tiebreak: 18,
    king_danger_pct: 250,
    king_k: 150,
    zone_order: 3250,
    sac_bonus: 20,
    after_sac_bonus: 180,
    chain_bonus: 190,
    pressure_start: 14,
    pressure_step: 15,
    gambit_bonus: 40,
    sac_floor: -430,
    sac_full_units: 2,
    sac_partial_hi: 76,
    sac_partial_lo: 35,
    tal_sacs: true,
    check_see: 320,
    sac_ext_depth: 3,
    material_preservation: 15,
};
