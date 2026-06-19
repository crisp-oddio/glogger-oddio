//! Static catalog mapping known Word of Power effect names to a category and
//! level, derived from https://wiki.projectgorgon.com/wiki/Words_of_Power.
//!
//! The game groups word-of-power scrolls into six tiers (levels 0/3/5/7/9/19);
//! several effect names recur across multiple tiers (e.g. "Super Jumping"
//! appears at levels 3/5/7/9 with a longer duration each time). The discovery
//! log line gives us only the effect name, not which tier scroll produced it,
//! so each name below is pinned to the **lowest** tier it appears at on the
//! wiki — a deterministic, if approximate, single level per name.

use once_cell::sync::Lazy;
use std::collections::HashMap;

pub const UNKNOWN_CATEGORY: &str = "Unknown";

/// (power name, category, level)
const CATALOG: &[(&str, &str, u32)] = &[
    // Level 0
    ("Teleport to Crypt", "Teleports", 0),
    ("Teleport to Eltibule Keep", "Teleports", 0),
    ("Teleport to Goblin Dungeon", "Teleports", 0),
    ("Teleport to Serbule", "Teleports", 0),
    ("Weak Life Regeneration", "Combat", 0),
    ("Weak Max-Power Boost", "Combat", 0),
    ("Cure Bovinity", "Utility", 0),
    ("Increase inventory", "Utility", 0),
    ("Fast Swimmer", "Utility", 0),
    ("Hold your Breath", "Utility", 0),
    ("Instant Death", "Debuffs", 0),
    ("Anemia", "Debuffs", 0),
    ("Fear of Water", "Debuffs", 0),
    // Level 3
    ("Teleport to Kur Mountains", "Teleports", 3),
    ("Teleport to Crystal Cavern", "Teleports", 3),
    ("Teleport to Hogan's Keep", "Teleports", 3),
    ("Modest Life Regeneration", "Combat", 3),
    ("Modest Max Power Boost", "Combat", 3),
    ("Iron Skin", "Combat", 3),
    ("Whiney Voice", "Combat", 3),
    ("Mind Shield", "Combat", 3),
    ("Flapskull Incubation", "Combat", 3),
    ("Unarmed Knowledge", "Experience", 3),
    ("Sword Knowledge", "Experience", 3),
    ("Cure Arachnia", "Utility", 3),
    ("Super Jumping", "Utility", 3),
    ("Unnatural Gravity", "Debuffs", 3),
    ("Leprosy", "Debuffs", 3),
    // Level 5
    ("Teleport to Sun Vale", "Teleports", 5),
    ("Teleport to Red Wing Casino", "Teleports", 5),
    ("Perfect Health", "Combat", 5),
    ("Diamond Skin", "Combat", 5),
    ("Archery Master", "Combat", 5),
    ("Impressive Life Regeneration", "Combat", 5),
    ("Impressive Max-Power Boost", "Combat", 5),
    ("Slug Incubation", "Combat", 5),
    ("Resurrect", "Combat", 5),
    ("Alchemy Knowledge", "Experience", 5),
    ("Cure Deeriness", "Utility", 5),
    ("Cure Disease", "Utility", 5),
    ("Cure Pigification", "Utility", 5),
    ("Flight", "Utility", 5),
    ("Human Illusion", "Utility", 5),
    ("Preparedness", "Utility", 5),
    ("Increased Inventory", "Utility", 5),
    ("Rare Loot", "Utility", 5),
    ("Lame Leg", "Inutile", 5),
    // Level 7
    ("Unreasonable Life Regeneration", "Combat", 7),
    ("Unreasonable Max-Power Boost", "Combat", 7),
    ("Poison Slug Incubation", "Combat", 7),
    ("Hateable Face", "Combat", 7),
    ("Deer Form", "Combat", 7),
    ("Experience Boost", "Combat", 7),
    ("Resurrection", "Utility", 7),
    ("Teleport to Ilmari", "Utility", 7),
    ("Epic Loot", "Utility", 7),
    ("Elf Illusion", "Utility", 7),
    ("Camel Essence", "Utility", 7),
    // Level 9
    ("Teleport to Trollvale (Fae Realm)", "Teleports", 9),
    ("Disgusting Body Odor", "Combat", 9),
    ("Unimaginable Max-Power Boost", "Utility", 9),
    ("Rakshasa Illusion", "Utility", 9),
    // Level 19
    ("Teleport to Vidaria", "Teleports", 19),
    ("Teleport into the Rubywall", "Teleports", 19),
    ("Teleport to Povus", "Teleports", 19),
    ("Everlasting Body Odor", "Combat", 19),
    ("Healing Funnel", "Combat", 19),
    ("Elemental Immunity", "Combat", 19),
    ("Empower Steed", "Utility", 19),
    ("Inner Flame", "Utility", 19),
];

static LOOKUP: Lazy<HashMap<String, (&'static str, u32)>> = Lazy::new(|| {
    CATALOG
        .iter()
        .map(|(name, category, level)| (name.to_lowercase(), (*category, *level)))
        .collect()
});

/// Look up the (category, level) for a known word-of-power effect name.
/// Returns `(UNKNOWN_CATEGORY, None)` for names not in the catalog (e.g.
/// manually-added or not-yet-cataloged words).
pub fn lookup(power_name: &str) -> (&'static str, Option<u32>) {
    match LOOKUP.get(power_name.trim().to_lowercase().as_str()) {
        Some((category, level)) => (category, Some(*level)),
        None => (UNKNOWN_CATEGORY, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_name_resolves_category_and_level() {
        assert_eq!(lookup("Unnatural Gravity"), ("Debuffs", Some(3)));
        assert_eq!(lookup("  super jumping  "), ("Utility", Some(3)));
    }

    #[test]
    fn unknown_name_returns_unknown_category() {
        assert_eq!(lookup("Totally Made Up Power"), (UNKNOWN_CATEGORY, None));
    }
}
