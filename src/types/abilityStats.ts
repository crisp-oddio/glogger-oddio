// Mirrors the Rust `game_data::ability_stats` output structs returned by the
// `compute_ability_build_stats` Tauri command. serde serializes Rust field names
// as-is (snake_case), so these interfaces use snake_case to match the wire shape.

/** One contributing gear-mod line attached to a computed stat. */
export interface ContributionLine {
  source: string
  label: string
  value: number
  display_type: string
  /** Formula bucket: added | base_mod | damage_mod | crit_mod | delta | mod | delta_base */
  bucket: string
}

/** A base value and its build-modified effective value, with responsible mods. */
export interface ValueBreakdown {
  base: number
  effective: number
  added: number
  base_pct: number
  all_pct: number
  contributions: ContributionLine[]
  dormant_activated: boolean
}

export interface DotBreakdown {
  damage_type: string | null
  num_ticks: number
  duration: number | null
  per_tick: ValueBreakdown
  total_base: number
  total_effective: number
}

export interface SpecialValueBreakdown {
  label: string | null
  suffix: string | null
  display_type: string | null
  value: ValueBreakdown
}

export interface AbilityBuildStats {
  damage_type: string | null
  direct_damage: ValueBreakdown | null
  dots: DotBreakdown[]
  special_values: SpecialValueBreakdown[]
  power_cost: ValueBreakdown | null
  rage_cost: ValueBreakdown | null
  any_modified: boolean
}

/** A single assigned build mod, as passed to `compute_ability_build_stats`. */
export interface AbilityModRef {
  power_name: string
  tier: number
  slot_label: string
}

/** Whether a value breakdown was actually changed by the build's mods. */
export function isValueModified(v: ValueBreakdown): boolean {
  return v.contributions.length > 0 && Math.abs(v.effective - v.base) > 1e-9
}
