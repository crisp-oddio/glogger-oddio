//! Pure formula engine that folds a build's assigned gear-mod contributions into
//! an ability's combat stats, producing the same kind of computed numbers the
//! in-game ability tooltip shows (damage, DoTs, heals/restores, costs).
//!
//! Project: Gorgon's direct-damage formula (wiki: Combat):
//! ```text
//! final = base * (1 + ΣModBaseDamage% + ΣModDamage%) + ΣDeltaDamage * (1 + ΣModDamage%)
//! ```
//! DoTs and SpecialValues share the same base-value + delta/mod shape, so a single
//! [`apply_formula`] helper drives all of them (with no base-only % bucket for those).
//!
//! All game-formula logic lives here so it can be unit-tested without the CDN or a
//! running app; the Tauri command in `cdn_commands.rs` only does data extraction.

use serde::Serialize;
use std::collections::HashSet;

use super::abilities::{CombatStats, DotEffect, SpecialValue};

/// A single resolved gear-mod effect that may feed one of an ability's attribute
/// buckets. `token` is the raw attribute key (e.g. `MOD_SKILL_UNARMED`).
#[derive(Debug, Clone)]
pub struct ModEffect {
    pub token: String,
    pub value: f64,
    /// Display source, e.g. `"Martial … of Deadly Fists (Hands)"`.
    pub source: String,
    /// Human-readable attribute label, e.g. `"Unarmed Base Damage %"`.
    pub label: String,
    /// Attribute `DisplayType` (`AsBuffMod`, `AsBuffDelta`, …) for frontend formatting.
    pub display_type: String,
}

/// One contributing mod line attached to a computed stat, for the breakdown UI.
#[derive(Debug, Serialize, Clone)]
pub struct ContributionLine {
    pub source: String,
    pub label: String,
    pub value: f64,
    pub display_type: String,
    /// Which formula bucket this fed: `added` | `base_mod` | `damage_mod` |
    /// `crit_mod` | `delta` | `mod` | `delta_base`.
    pub bucket: String,
}

/// A base value and its build-modified effective value, with the mods responsible.
#[derive(Debug, Serialize, Clone, Default)]
pub struct ValueBreakdown {
    pub base: f64,
    pub effective: f64,
    /// Flat amount added by mods (before/with the % multipliers).
    pub added: f64,
    /// Base-only percentage total (fraction, e.g. 0.45 == +45%). Damage only.
    pub base_pct: f64,
    /// All-damage percentage total (fraction).
    pub all_pct: f64,
    pub contributions: Vec<ContributionLine>,
    /// True when the base was zero but a mod made it non-zero (dormant → active).
    pub dormant_activated: bool,
}

impl ValueBreakdown {
    /// Whether any mod changed this value (drives "show the with-build column").
    pub fn is_modified(&self) -> bool {
        !self.contributions.is_empty() && (self.effective - self.base).abs() > f64::EPSILON
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct DotBreakdown {
    pub damage_type: Option<String>,
    pub num_ticks: f64,
    pub duration: Option<f64>,
    pub per_tick: ValueBreakdown,
    pub total_base: f64,
    pub total_effective: f64,
}

#[derive(Debug, Serialize, Clone)]
pub struct SpecialValueBreakdown {
    pub label: Option<String>,
    pub suffix: Option<String>,
    pub display_type: Option<String>,
    pub value: ValueBreakdown,
}

/// Full computed combat picture for one ability under a given build.
#[derive(Debug, Serialize, Clone, Default)]
pub struct AbilityBuildStats {
    pub damage_type: Option<String>,
    pub direct_damage: Option<ValueBreakdown>,
    pub dots: Vec<DotBreakdown>,
    pub special_values: Vec<SpecialValueBreakdown>,
    pub power_cost: Option<ValueBreakdown>,
    pub rage_cost: Option<ValueBreakdown>,
    /// True if any component is affected by the build's mods.
    pub any_modified: bool,
}

/// The PG combat formula. `base_pct` is the base-only multiplier bucket (damage only);
/// `all_pct` multiplies both the base and the flat `added` amount.
fn apply_formula(base: f64, added: f64, base_pct: f64, all_pct: f64) -> f64 {
    base * (1.0 + base_pct + all_pct) + added * (1.0 + all_pct)
}

/// Sum the values of every [`ModEffect`] whose token is in `tokens`, pushing each as a
/// labeled contribution line into `out`.
fn collect_bucket(
    mods: &[ModEffect],
    tokens: &[String],
    bucket: &str,
    out: &mut Vec<ContributionLine>,
) -> f64 {
    if tokens.is_empty() {
        return 0.0;
    }
    let set: HashSet<&str> = tokens.iter().map(|s| s.as_str()).collect();
    let mut total = 0.0;
    for m in mods {
        if set.contains(m.token.as_str()) {
            total += m.value;
            out.push(ContributionLine {
                source: m.source.clone(),
                label: m.label.clone(),
                value: m.value,
                display_type: m.display_type.clone(),
                bucket: bucket.to_string(),
            });
        }
    }
    total
}

/// Compute the direct-damage breakdown for an ability + mods, using the full PG formula
/// (added / base-mult / damage-mult buckets). Crit-damage mods are surfaced as
/// informational contribution lines but excluded from the non-crit `effective`.
fn direct_damage(stats: &CombatStats, mods: &[ModEffect]) -> Option<ValueBreakdown> {
    let base = stats.damage? as f64;
    let mut contributions = Vec::new();
    let added = collect_bucket(mods, &stats.attributes_that_delta_damage, "added", &mut contributions);
    let base_pct = collect_bucket(mods, &stats.attributes_that_mod_base_damage, "base_mod", &mut contributions);
    let all_pct = collect_bucket(mods, &stats.attributes_that_mod_damage, "damage_mod", &mut contributions);
    // Crit mods are informational only (don't change the listed hit).
    collect_bucket(mods, &stats.attributes_that_mod_crit_damage, "crit_mod", &mut contributions);

    let effective = apply_formula(base, added, base_pct, all_pct);
    Some(ValueBreakdown {
        base,
        effective,
        added,
        base_pct,
        all_pct,
        dormant_activated: base == 0.0 && effective != 0.0,
        contributions,
    })
}

/// DoT damage is *indirect* damage, governed only by indirect-damage modifiers — the DoT's
/// own per-ability tokens plus the generic per-damage-type and universal indirect attributes
/// (`BOOST_/MOD_<TYPE>_INDIRECT`, `*_UNIVERSAL_INDIRECT`, applied globally by the DoT's
/// damage type). Base-skill-damage % and the ability's *direct*-damage modifiers do NOT
/// affect ticks.
fn dot_breakdown(dot: &DotEffect, mods: &[ModEffect]) -> DotBreakdown {
    let base = dot.damage_per_tick as f64;
    let mut contributions = Vec::new();

    let dtype = dot.damage_type.as_deref();
    let mut flat_tokens = dot.attributes_that_delta.clone();
    flat_tokens.extend(indirect_tokens("BOOST", dtype));
    let mut pct_tokens = dot.attributes_that_mod.clone();
    pct_tokens.extend(indirect_tokens("MOD", dtype));

    let added = collect_bucket(mods, &flat_tokens, "delta", &mut contributions);
    let all_pct = collect_bucket(mods, &pct_tokens, "mod", &mut contributions);
    let per_tick_eff = apply_formula(base, added, 0.0, all_pct);
    let ticks = dot.num_ticks as f64;
    DotBreakdown {
        damage_type: dot.damage_type.clone(),
        num_ticks: ticks,
        duration: dot.duration.map(|d| d as f64),
        total_base: base * ticks,
        total_effective: per_tick_eff * ticks,
        per_tick: ValueBreakdown {
            base,
            effective: per_tick_eff,
            added,
            base_pct: 0.0,
            all_pct,
            dormant_activated: base == 0.0 && per_tick_eff != 0.0,
            contributions,
        },
    }
}

/// Generic indirect-damage attribute tokens that apply to any DoT of `damage_type`, plus
/// the universal (all-type) indirect modifier. `prefix` is `BOOST` (flat per-tick) or
/// `MOD` (percent). e.g. ("BOOST", Some("Psychic")) →
/// ["BOOST_UNIVERSAL_INDIRECT", "BOOST_PSYCHIC_INDIRECT"].
fn indirect_tokens(prefix: &str, damage_type: Option<&str>) -> Vec<String> {
    let mut v = vec![format!("{prefix}_UNIVERSAL_INDIRECT")];
    if let Some(dt) = damage_type {
        v.push(format!("{prefix}_{}_INDIRECT", dt.to_uppercase()));
    }
    v
}

/// Damage-type names whose uppercase form matches the `*_<TYPE>_INDIRECT` attribute tokens.
const DAMAGE_TYPES: &[&str] = &[
    "Acid", "Cold", "Crushing", "Darkness", "Demonic", "Divine", "Electricity", "Fire",
    "Fungus", "Nature", "Piercing", "Poison", "Psychic", "Seafood", "Slashing", "Sonic",
    "Trauma",
];

/// True when a special value represents indirect (damage-over-time) damage — detected by a
/// DoT/indirect attribute token in its own arrays (`*ABILITYDOT*`, `*_INDIRECT`). This keeps
/// heals/armor-over-time (which use `*HEAL*`/`*ARMOR*` tokens) from picking up damage mods.
fn sv_is_indirect_damage(sv: &SpecialValue) -> bool {
    sv.attributes_that_delta_base
        .iter()
        .chain(&sv.attributes_that_delta)
        .chain(&sv.attributes_that_mod)
        .any(|t| t.contains("ABILITYDOT") || t.contains("_INDIRECT"))
}

/// Find the damage type named in a special value's label/suffix (e.g. "Trauma damage" →
/// "Trauma") so the matching generic indirect tokens can be selected. Falls back to `None`
/// (universal-only) when no type is named.
fn parse_damage_type(sv: &SpecialValue) -> Option<String> {
    let text = format!(
        "{} {}",
        sv.label.as_deref().unwrap_or(""),
        sv.suffix.as_deref().unwrap_or("")
    )
    .to_lowercase();
    DAMAGE_TYPES
        .iter()
        .find(|t| text.contains(&t.to_lowercase()))
        .map(|t| t.to_string())
}

fn special_value_breakdown(sv: &SpecialValue, mods: &[ModEffect]) -> SpecialValueBreakdown {
    let base = sv.value as f64;
    let mut contributions = Vec::new();

    // A special value that carries a DoT/indirect token (e.g. reflect-style "they suffer X
    // Trauma damage over time") is indirect damage, so the generic per-damage-type and
    // universal indirect modifiers apply on top of its own tokens — same rule as DoTs.
    let mut delta_tokens = sv.attributes_that_delta.clone();
    let mut mod_tokens = sv.attributes_that_mod.clone();
    if sv_is_indirect_damage(sv) {
        let dtype = parse_damage_type(sv);
        delta_tokens.extend(indirect_tokens("BOOST", dtype.as_deref()));
        mod_tokens.extend(indirect_tokens("MOD", dtype.as_deref()));
    }

    // DeltaBase adds to the base before the multiplier; Delta adds afterward. Both end up
    // multiplied by (1 + mod%), so they fold into `added` for the arithmetic but keep
    // distinct bucket labels for the breakdown display.
    let delta_base = collect_bucket(mods, &sv.attributes_that_delta_base, "delta_base", &mut contributions);
    let delta = collect_bucket(mods, &delta_tokens, "delta", &mut contributions);
    let mod_pct = collect_bucket(mods, &mod_tokens, "mod", &mut contributions);
    let effective = apply_formula(base + delta_base, delta, 0.0, mod_pct);
    SpecialValueBreakdown {
        label: sv.label.clone(),
        suffix: sv.suffix.clone(),
        display_type: sv.display_type.clone(),
        value: ValueBreakdown {
            base,
            effective,
            added: delta_base + delta,
            base_pct: 0.0,
            all_pct: mod_pct,
            dormant_activated: base == 0.0 && effective != 0.0,
            contributions,
        },
    }
}

/// Cost breakdown (power/rage): `(base + ΣDelta) * (1 + ΣMod%)`.
fn cost_breakdown(
    base: Option<f32>,
    delta_tokens: &[String],
    mod_tokens: &[String],
    mods: &[ModEffect],
) -> Option<ValueBreakdown> {
    let base = base? as f64;
    let mut contributions = Vec::new();
    let added = collect_bucket(mods, delta_tokens, "delta", &mut contributions);
    let mod_pct = collect_bucket(mods, mod_tokens, "mod", &mut contributions);
    let effective = (base + added) * (1.0 + mod_pct);
    Some(ValueBreakdown {
        base,
        effective,
        added,
        base_pct: 0.0,
        all_pct: mod_pct,
        dormant_activated: false,
        contributions,
    })
}

/// Fold a build's `mods` into `stats`, producing the full computed picture.
pub fn compute(
    stats: &CombatStats,
    damage_type: Option<String>,
    mods: &[ModEffect],
) -> AbilityBuildStats {
    let direct = direct_damage(stats, mods);
    let dots: Vec<DotBreakdown> = stats.dots.iter().map(|d| dot_breakdown(d, mods)).collect();
    let special_values: Vec<SpecialValueBreakdown> = stats
        .special_values
        .iter()
        .map(|s| special_value_breakdown(s, mods))
        .collect();
    let power_cost = cost_breakdown(
        stats.power_cost,
        &stats.attributes_that_delta_power_cost,
        &stats.attributes_that_mod_power_cost,
        mods,
    );
    let rage_cost = cost_breakdown(
        stats.rage_cost,
        &stats.attributes_that_delta_rage,
        &stats.attributes_that_mod_rage,
        mods,
    );

    let any_modified = direct.as_ref().is_some_and(|d| d.is_modified())
        || dots.iter().any(|d| d.per_tick.is_modified())
        || special_values.iter().any(|s| s.value.is_modified())
        || power_cost.as_ref().is_some_and(|c| c.is_modified())
        || rage_cost.as_ref().is_some_and(|c| c.is_modified());

    AbilityBuildStats {
        damage_type,
        direct_damage: direct,
        dots,
        special_values,
        power_cost,
        rage_cost,
        any_modified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(token: &str, value: f64) -> ModEffect {
        ModEffect {
            token: token.to_string(),
            value,
            source: format!("Mod {token}"),
            label: token.to_string(),
            display_type: "AsBuffMod".to_string(),
        }
    }

    /// Punch: base 6, +45% base (MOD_SKILL_UNARMED), +10 flat (BOOST_ABILITY_PUNCH),
    /// +50% all (MOD_ABILITY_PUNCH) → 6×(1+0.45+0.50) + 10×(1+0.50) = 11.7 + 15 = 26.7.
    #[test]
    fn punch_direct_damage_matches_pg_formula() {
        let stats = CombatStats {
            damage: Some(6.0),
            attributes_that_delta_damage: vec!["BOOST_ABILITY_PUNCH".into()],
            attributes_that_mod_base_damage: vec!["MOD_SKILL_UNARMED".into()],
            attributes_that_mod_damage: vec!["MOD_ABILITY_PUNCH".into()],
            ..Default::default()
        };
        let mods = vec![
            m("MOD_SKILL_UNARMED", 0.45),
            m("BOOST_ABILITY_PUNCH", 10.0),
            m("MOD_ABILITY_PUNCH", 0.50),
        ];
        let out = compute(&stats, Some("Crushing".into()), &mods);
        let dd = out.direct_damage.unwrap();
        assert!((dd.base - 6.0).abs() < 1e-6);
        assert!((dd.effective - 26.7).abs() < 1e-6, "got {}", dd.effective);
        assert!((dd.added - 10.0).abs() < 1e-6);
        assert!((dd.base_pct - 0.45).abs() < 1e-6);
        assert!((dd.all_pct - 0.50).abs() < 1e-6);
        assert_eq!(dd.contributions.len(), 3);
        assert!(out.any_modified);
    }

    /// Unrelated mods must not touch an ability whose buckets don't list their tokens.
    #[test]
    fn unrelated_mods_do_not_apply() {
        let stats = CombatStats {
            damage: Some(20.0),
            attributes_that_mod_base_damage: vec!["MOD_SKILL_SWORD".into()],
            ..Default::default()
        };
        let out = compute(&stats, None, &[m("MOD_SKILL_UNARMED", 0.99)]);
        let dd = out.direct_damage.unwrap();
        assert!((dd.effective - 20.0).abs() < 1e-6);
        assert!(!dd.is_modified());
        assert!(!out.any_modified);
    }

    /// A heal SpecialValue: base 25, +5 flat heal → 30.
    #[test]
    fn special_value_heal_adds_flat() {
        let stats = CombatStats {
            special_values: vec![SpecialValue {
                label: Some("Restore".into()),
                suffix: Some("Health to You and Nearby Allies".into()),
                value: 25.0,
                attributes_that_delta: vec!["BOOST_MAJORHEAL_SENDER".into()],
                ..Default::default()
            }],
            ..Default::default()
        };
        let out = compute(&stats, None, &[m("BOOST_MAJORHEAL_SENDER", 5.0)]);
        let sv = &out.special_values[0];
        assert!((sv.value.base - 25.0).abs() < 1e-6);
        assert!((sv.value.effective - 30.0).abs() < 1e-6);
        assert_eq!(sv.label.as_deref(), Some("Restore"));
    }

    /// A DoT expressed as a SpecialValue (reflect-style, e.g. Tough Hoof's "X Trauma damage")
    /// gets generic indirect modifiers too, but not base-skill-damage.
    #[test]
    fn special_value_dot_gets_generic_indirect() {
        let stats = CombatStats {
            special_values: vec![SpecialValue {
                label: Some("For 8 seconds, each time target attacks you, they suffer".into()),
                suffix: Some("Trauma damage".into()),
                value: 50.0,
                attributes_that_delta: vec!["BOOST_ABILITYDOT_TOUGHHOOF".into()],
                ..Default::default()
            }],
            ..Default::default()
        };
        let mods = vec![
            m("BOOST_ABILITYDOT_TOUGHHOOF", 30.0), // ability-specific DoT flat
            m("MOD_TRAUMA_INDIRECT", 0.25),        // generic % indirect (Trauma)
            m("MOD_SKILL_UNARMED", 1.00),          // base-skill: must NOT apply
        ];
        let out = compute(&stats, None, &mods);
        let sv = &out.special_values[0];
        // (50 + 30) × (1 + 0.25) = 100
        assert!((sv.value.effective - 100.0).abs() < 1e-6, "got {}", sv.value.effective);
    }

    /// A dormant SpecialValue (base 0, SkipIfZero) lights up when a mod feeds its token.
    #[test]
    fn dormant_special_value_activates() {
        let stats = CombatStats {
            special_values: vec![SpecialValue {
                label: Some("Restore".into()),
                value: 0.0,
                skip_if_zero: true,
                attributes_that_delta: vec!["BOOST_HEALINGMIST_TSYS_HEALTH_SENDER".into()],
                ..Default::default()
            }],
            ..Default::default()
        };
        let out = compute(&stats, None, &[m("BOOST_HEALINGMIST_TSYS_HEALTH_SENDER", 12.0)]);
        let sv = &out.special_values[0];
        assert!((sv.value.effective - 12.0).abs() < 1e-6);
        assert!(sv.value.dormant_activated);
        assert!(out.any_modified);
    }

    /// DoT: per-tick 10 over 4 ticks, +50% mod → per-tick 15, total 60.
    #[test]
    fn dot_applies_mod_to_each_tick() {
        let stats = CombatStats {
            dots: vec![DotEffect {
                damage_per_tick: 10.0,
                num_ticks: 4.0,
                duration: Some(8.0),
                damage_type: Some("Fire".into()),
                attributes_that_mod: vec!["MOD_ABILITYDOT_BARRAGE".into()],
                ..Default::default()
            }],
            ..Default::default()
        };
        let out = compute(&stats, None, &[m("MOD_ABILITYDOT_BARRAGE", 0.5)]);
        let dot = &out.dots[0];
        assert!((dot.per_tick.effective - 15.0).abs() < 1e-6);
        assert!((dot.total_effective - 60.0).abs() < 1e-6);
        assert!((dot.total_base - 40.0).abs() < 1e-6);
    }

    /// Mindworm: a DoT-only ability (Psychic, 140/tick × 4). DoT damage is *indirect*, so it
    /// is moved only by indirect modifiers — never by base-skill-damage % or the ability's
    /// direct-damage mods (which apply to the absent direct hit).
    #[test]
    fn dot_uses_indirect_mods_not_direct_or_base_skill() {
        let stats = CombatStats {
            damage: None,
            attributes_that_delta_damage: vec!["BOOST_ABILITY_MINDWORM".into()],
            attributes_that_mod_base_damage: vec!["MOD_SKILL_MENTALISM".into()],
            attributes_that_mod_damage: vec!["MOD_ABILITY_MINDWORM".into()],
            dots: vec![DotEffect {
                damage_per_tick: 140.0,
                num_ticks: 4.0,
                duration: Some(8.0),
                damage_type: Some("Psychic".into()),
                attributes_that_delta: vec!["BOOST_ABILITYDOT_MINDWORM".into()],
                ..Default::default()
            }],
            ..Default::default()
        };

        // Direct + base-skill mods must NOT touch the DoT.
        let direct_only = vec![
            m("MOD_SKILL_MENTALISM", 1.05),
            m("BOOST_ABILITY_MINDWORM", 202.0),
            m("MOD_ABILITY_MINDWORM", 1.30),
        ];
        let out = compute(&stats, Some("Psychic".into()), &direct_only);
        assert!(out.direct_damage.is_none(), "no direct Damage on Mindworm");
        let dot = &out.dots[0];
        assert!((dot.per_tick.effective - 140.0).abs() < 1e-6, "direct/base mods must not move the DoT, got {}", dot.per_tick.effective);
        assert!(!out.any_modified);

        // Indirect mods DO apply: +140 flat (DoT token), +20% Psychic-indirect, +10% universal.
        // (140 + 140) × (1 + 0.20 + 0.10) = 280 × 1.30 = 364/tick → 1456 total.
        let indirect = vec![
            m("BOOST_ABILITYDOT_MINDWORM", 140.0),
            m("MOD_PSYCHIC_INDIRECT", 0.20),
            m("MOD_UNIVERSAL_INDIRECT", 0.10),
        ];
        let out2 = compute(&stats, Some("Psychic".into()), &indirect);
        let dot2 = &out2.dots[0];
        assert!((dot2.per_tick.effective - 364.0).abs() < 1e-6, "got {}", dot2.per_tick.effective);
        assert!((dot2.total_effective - 1456.0).abs() < 1e-6, "got {}", dot2.total_effective);
        assert!(out2.any_modified);
    }

    /// Power cost reduction: base 16, -4 delta → 12.
    #[test]
    fn power_cost_delta_reduces() {
        let stats = CombatStats {
            power_cost: Some(16.0),
            attributes_that_delta_power_cost: vec!["DELTA_POWER_COST_X".into()],
            ..Default::default()
        };
        let out = compute(&stats, None, &[m("DELTA_POWER_COST_X", -4.0)]);
        let pc = out.power_cost.unwrap();
        assert!((pc.effective - 12.0).abs() < 1e-6);
    }
}
