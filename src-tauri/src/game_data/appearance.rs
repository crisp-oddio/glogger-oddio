//! Parser for Project: Gorgon item `EquipAppearance` strings.
//!
//! These strings map an equipped item to the 3D mesh + material + dye config
//! the game renders. glogger's Model Viewer parses them and joins the result
//! against the extracted asset catalog (see `tools/model_extractor/`).
//!
//! Grammar (informal), a `;`-separated list of directives at the top level
//! (semicolons inside `(...)` are nested and do NOT split):
//!
//! ```text
//!   @Chest=@eq-{sex}2-chest-leather-01(^Armor={sex}2-body-leather-01%DYE%);Bra=off
//!   @MainHand=eq-x-knife1;MainHandEquip=Knife
//!   @Head=SantaHatBlue
//!   @Necklace=none;SkinColor1=77ccff
//! ```
//!
//! - `@<Slot>=<value>` is a **slot directive**.
//!   - `@<meshKey>(^<props>)` — dyeable armor. `props` are `;`-separated;
//!     `Armor=<materialKey>` names the dye material, `DyeOrder=CAB` the
//!     channel order, and a `%DYE%` marker means the player's dyes inject here.
//!   - bare `<meshKey>` (e.g. `eq-x-knife1`, `SantaHatBlue`) — a static mesh
//!     with no dye override.
//!   - `none` — the slot renders nothing.
//!   - `[sex=m:A|sex=f:B]` — a sex conditional; the matching branch is used.
//! - `<Key>=<Value>` (no leading `@`) is a **global flag** (`Hair=Off`,
//!   `Bra=off`, `OffHandEquip=Shield`, `SkinColor1=...`, ...).
//!
//! `{sex}` placeholders are substituted with `m`/`f` before parsing.

use serde::Serialize;
use std::collections::BTreeMap;

/// One `@Slot=...` directive from an appearance string.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppearanceDirective {
    /// Slot name as written, e.g. `Chest`, `MainHand`, `RealHands`, `Racial`.
    pub slot: String,
    /// GameObject/mesh key (catalog `appearances` / `weapons` key), or `None`
    /// when the directive is `none`.
    pub mesh_key: Option<String>,
    /// Dye material key from `^Armor=` (catalog `materials` key), if any.
    pub material_key: Option<String>,
    /// Dye channel order from `^DyeOrder=` (e.g. `CAB`), if specified.
    pub dye_order: Option<String>,
    /// Whether this piece is dyeable (a `%DYE%` marker was present).
    pub dyeable: bool,
    /// Remaining `^` props (e.g. `GlowEnabled=y`, `Skin=...`).
    pub props: BTreeMap<String, String>,
}

/// A fully parsed appearance string.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedAppearance {
    pub directives: Vec<AppearanceDirective>,
    /// Global flags (non-`@` directives): `Hair=Off`, `OffHandEquip=Shield`, …
    pub flags: BTreeMap<String, String>,
    /// Which source field was used: `"EquipAppearance2"` or `"EquipAppearance"`.
    pub source_field: &'static str,
}

/// Split `s` on top-level `sep`, treating `(...)` and `[...]` as nested (their
/// separators are ignored).
fn split_top_level(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '(' | '[' => {
                depth += 1;
                cur.push(c);
            }
            ')' | ']' => {
                depth -= 1;
                cur.push(c);
            }
            _ if c == sep && depth == 0 => {
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Resolve a `[sex=m:A|sex=f:B]` conditional to the branch for `sex`.
/// Returns the raw string unchanged if it isn't a conditional.
fn resolve_sex_conditional(value: &str, sex: char) -> String {
    let inner = match value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        Some(i) => i,
        None => return value.to_string(),
    };
    for branch in inner.split('|') {
        if let Some((cond, body)) = branch.split_once(':') {
            // cond like "sex=m"
            if let Some(want) = cond.trim().strip_prefix("sex=") {
                if want.trim().chars().next() == Some(sex) {
                    return body.to_string();
                }
            }
        }
    }
    // No matching branch → nothing to render.
    "none".to_string()
}

/// Parse the value of a slot directive (`@Slot=<value>`).
fn parse_slot_value(slot: &str, value: &str, sex: char) -> AppearanceDirective {
    let value = resolve_sex_conditional(value.trim(), sex);
    let mut d = AppearanceDirective {
        slot: slot.to_string(),
        mesh_key: None,
        material_key: None,
        dye_order: None,
        dyeable: false,
        props: BTreeMap::new(),
    };

    if value.eq_ignore_ascii_case("none") || value.is_empty() {
        return d;
    }

    // The mesh value may optionally lead with '@', and may carry a
    // "(^Armor=…;DyeOrder=…%DYE%)" material clause. Both forms occur:
    //   @Chest=@eq-m2-chest-leather-01(^Armor=m2-body-leather-01%DYE%)
    //   @WerewolfFeet=eq-w-plate-feet-1(^Armor=WerewolfPlate1%DYE%)   (no inner @)
    let body = value.strip_prefix('@').unwrap_or(&value);
    if let Some(open) = body.find('(') {
        d.mesh_key = Some(body[..open].to_string());
        let close = body.rfind(')').unwrap_or(body.len());
        let inner = &body[open + 1..close.min(body.len())];
        // inner starts with '^'; a '%DYE%' marker may appear anywhere.
        let inner = inner.strip_prefix('^').unwrap_or(inner);
        d.dyeable = inner.contains("%DYE%");
        let inner = inner.replace("%DYE%", "");
        for prop in split_top_level(&inner, ';') {
            let prop = prop.trim();
            if prop.is_empty() {
                continue;
            }
            if let Some((k, v)) = prop.split_once('=') {
                match k {
                    "Armor" | "Skin" => d.material_key = Some(v.to_string()),
                    "DyeOrder" => d.dye_order = Some(v.to_string()),
                    _ => {
                        d.props.insert(k.to_string(), v.to_string());
                    }
                }
            }
        }
    } else {
        // Bare mesh, no material clause: "eq-x-knife1", "SantaHatBlue".
        d.mesh_key = Some(body.to_string());
    }
    d
}

/// Parse a raw appearance string for the given `sex` (`'m'` or `'f'`).
pub fn parse_equip_appearance(raw: &str, sex: char) -> ParsedAppearance {
    parse_with_source(raw, sex, "EquipAppearance")
}

fn parse_with_source(raw: &str, sex: char, source_field: &'static str) -> ParsedAppearance {
    let substituted = raw.replace("{sex}", &sex.to_string());
    let mut directives = Vec::new();
    let mut flags = BTreeMap::new();

    for token in split_top_level(&substituted, ';') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let (key, value) = match token.split_once('=') {
            Some(kv) => kv,
            None => continue,
        };
        if let Some(slot) = key.strip_prefix('@') {
            directives.push(parse_slot_value(slot, value, sex));
        } else {
            flags.insert(key.to_string(), value.to_string());
        }
    }

    ParsedAppearance {
        directives,
        flags,
        source_field,
    }
}

/// Choose the best appearance string (prefer the modern `EquipAppearance2`)
/// and parse it. Returns `None` if the item has neither field.
pub fn resolve_item_appearance(
    equip_appearance: Option<&str>,
    equip_appearance2: Option<&str>,
    sex: char,
) -> Option<ParsedAppearance> {
    if let Some(a2) = equip_appearance2.filter(|s| !s.trim().is_empty()) {
        return Some(parse_with_source(a2, sex, "EquipAppearance2"));
    }
    if let Some(a1) = equip_appearance.filter(|s| !s.trim().is_empty()) {
        return Some(parse_with_source(a1, sex, "EquipAppearance"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one<'a>(p: &'a ParsedAppearance, slot: &str) -> &'a AppearanceDirective {
        p.directives.iter().find(|d| d.slot == slot).expect("slot")
    }

    #[test]
    fn chest_leather_male() {
        let p = parse_equip_appearance(
            "@Chest=@eq-{sex}2-chest-leather-01(^Armor={sex}2-body-leather-01%DYE%)",
            'm',
        );
        let d = one(&p, "Chest");
        assert_eq!(d.mesh_key.as_deref(), Some("eq-m2-chest-leather-01"));
        assert_eq!(d.material_key.as_deref(), Some("m2-body-leather-01"));
        assert!(d.dyeable);
        assert_eq!(d.dye_order, None);
    }

    #[test]
    fn chest_female_substitution() {
        let p = parse_equip_appearance(
            "@Chest=@eq-{sex}2-chest-leather-01(^Armor={sex}2-body-leather-01%DYE%)",
            'f',
        );
        let d = one(&p, "Chest");
        assert_eq!(d.mesh_key.as_deref(), Some("eq-f2-chest-leather-01"));
        assert_eq!(d.material_key.as_deref(), Some("f2-body-leather-01"));
    }

    #[test]
    fn chest_with_dye_order_and_flag() {
        let p = parse_equip_appearance(
            "@Chest=@eq-{sex}-faeshian2-chest-01(^Armor=stalker-body;DyeOrder=CAB%DYE%);Bra=off",
            'm',
        );
        let d = one(&p, "Chest");
        assert_eq!(d.mesh_key.as_deref(), Some("eq-m-faeshian2-chest-01"));
        assert_eq!(d.material_key.as_deref(), Some("stalker-body"));
        assert_eq!(d.dye_order.as_deref(), Some("CAB"));
        assert!(d.dyeable);
        assert_eq!(p.flags.get("Bra").map(String::as_str), Some("off"));
    }

    #[test]
    fn head_with_hair_flag() {
        let p = parse_equip_appearance(
            "@Head=@eq-{sex}2-head-leather-01(^Armor={sex}2-head-leather-01%DYE%);Hair=Off",
            'm',
        );
        assert_eq!(
            one(&p, "Head").mesh_key.as_deref(),
            Some("eq-m2-head-leather-01")
        );
        assert_eq!(p.flags.get("Hair").map(String::as_str), Some("Off"));
    }

    #[test]
    fn material_clause_without_inner_at() {
        // Werewolf/cow plate use `@Slot=meshkey(^Armor=…%DYE%)` — no inner '@'.
        let p = parse_equip_appearance(
            "@Feet=none;@WerewolfFeet=eq-w-plate-feet-1(^Armor=WerewolfPlate1%DYE%)",
            'm',
        );
        let d = one(&p, "WerewolfFeet");
        assert_eq!(d.mesh_key.as_deref(), Some("eq-w-plate-feet-1"));
        assert_eq!(d.material_key.as_deref(), Some("WerewolfPlate1"));
        assert!(d.dyeable);
        assert_eq!(one(&p, "Feet").mesh_key, None);
    }

    #[test]
    fn glow_prop_inside_parens() {
        let p = parse_equip_appearance(
            "@Chest=@eq-{sex}-mage-chest-01(^Armor=mage-body%DYE%;GlowEnabled=y);Bra=off",
            'm',
        );
        let d = one(&p, "Chest");
        assert_eq!(d.material_key.as_deref(), Some("mage-body"));
        assert!(d.dyeable);
        assert_eq!(d.props.get("GlowEnabled").map(String::as_str), Some("y"));
    }

    #[test]
    fn weapon_mainhand() {
        let p = parse_equip_appearance("@MainHand=eq-x-knife1;MainHandEquip=Knife", 'm');
        let d = one(&p, "MainHand");
        assert_eq!(d.mesh_key.as_deref(), Some("eq-x-knife1"));
        assert_eq!(d.material_key, None);
        assert!(!d.dyeable);
        assert_eq!(p.flags.get("MainHandEquip").map(String::as_str), Some("Knife"));
    }

    #[test]
    fn shield_offhand() {
        let p = parse_equip_appearance("@OffHandShield=eq-x-shield1;OffHandEquip=Shield", 'm');
        assert_eq!(
            one(&p, "OffHandShield").mesh_key.as_deref(),
            Some("eq-x-shield1")
        );
    }

    #[test]
    fn named_head_appearance() {
        let p = parse_equip_appearance("@Head=SantaHatBlue", 'f');
        let d = one(&p, "Head");
        assert_eq!(d.mesh_key.as_deref(), Some("SantaHatBlue"));
        assert!(!d.dyeable);
    }

    #[test]
    fn none_slot() {
        let p = parse_equip_appearance("@Necklace=none;SkinColor1=77ccff", 'm');
        let d = one(&p, "Necklace");
        assert_eq!(d.mesh_key, None);
        assert_eq!(p.flags.get("SkinColor1").map(String::as_str), Some("77ccff"));
    }

    #[test]
    fn sex_conditional_realhands() {
        // female branch selected → real hands mesh; male branch → none
        let raw = "@Hands=@eq-{sex}-magecloth-hands-0(^Armor=magecloth-body%DYE%);@RealHands=[sex=m:none|sex=f:@eq-f-hands-0(^Skin=magecloth-body%DYE%)]";
        let pf = parse_equip_appearance(raw, 'f');
        let rh = one(&pf, "RealHands");
        assert_eq!(rh.mesh_key.as_deref(), Some("eq-f-hands-0"));
        assert_eq!(rh.material_key.as_deref(), Some("magecloth-body"));

        let pm = parse_equip_appearance(raw, 'm');
        assert_eq!(one(&pm, "RealHands").mesh_key, None);
    }

    #[test]
    fn resolve_prefers_appearance2() {
        let p = resolve_item_appearance(
            Some("@Chest=@eq-{sex}-old-01(^Armor=old%DYE%)"),
            Some("@Chest=@eq-{sex}2-new-01(^Armor={sex}2-new%DYE%)"),
            'm',
        )
        .unwrap();
        assert_eq!(p.source_field, "EquipAppearance2");
        assert_eq!(one(&p, "Chest").mesh_key.as_deref(), Some("eq-m2-new-01"));
    }

    #[test]
    fn resolve_falls_back_to_appearance1() {
        let p = resolve_item_appearance(Some("@OffHandShield=eq-x-shield1"), None, 'm').unwrap();
        assert_eq!(p.source_field, "EquipAppearance");
    }

    #[test]
    fn resolve_none_when_absent() {
        assert!(resolve_item_appearance(None, None, 'm').is_none());
    }
}
