// Types for the 3D Model Viewer feature. Field names are snake_case to match
// the Rust command payloads on the wire (same convention as ItemInfo).

export interface ModelViewerStatus {
  game_bundles_dir: string | null;
  cache_ready: boolean;
  cache_dir: string;
  appearance_count: number;
  material_count: number;
  weapon_count: number;
  source_bundle: string | null;
}

export interface ResolvedSlot {
  slot: string;
  mesh_key: string | null;
  material_key: string | null;
  dye_order: string | null;
  dyeable: boolean;
  /** Relative glb path in the model cache, or null if not extracted. */
  mesh_file: string | null;
  /** label → relative png filename (skin/mask1..3/normal/metallic/...). */
  textures: Record<string, string> | null;
  dye_channels: number;
  /** e.g. { _Color0: [r,g,b,a], _Color1: [...], ... } */
  default_colors: Record<string, number[]> | null;
  /** Weapon/held prop — oriented by bounding box rather than the body rotation. */
  is_weapon: boolean;
}

export interface ResolvedItemAppearance {
  item_id: number;
  item_name: string;
  sex: string;
  source_field: string;
  slots: ResolvedSlot[];
  flags: Record<string, string>;
  renderable: boolean;
  not_dyeable_keyword: boolean;
}

export interface BrowsableItem {
  id: number;
  name: string;
  icon_id: number | null;
  equip_slot: string | null;
  /** True if this item resolves to a mesh in the cache (many cosmetics don't). */
  has_model: boolean;
}

/** An item chosen for a loadout slot. */
export interface LoadoutEntry {
  ref: string;
  name: string;
  icon_id: number | null;
}

/** Equipment slots the Model Viewer shows (those with a 3D model). */
export const VIEWER_SLOTS: { id: string; label: string }[] = [
  { id: "Head", label: "Head" },
  { id: "Chest", label: "Chest" },
  { id: "Legs", label: "Legs" },
  { id: "Hands", label: "Hands" },
  { id: "Feet", label: "Feet" },
  { id: "MainHand", label: "Main Hand" },
  { id: "OffHand", label: "Off Hand" },
];

export interface ExtractionProgress {
  stage: string;
  message: string;
  done: boolean;
  ok: boolean;
}

/** Equipment slots that have a visible 3D appearance. */
export const APPEARANCE_SLOTS = ["Head", "Chest", "Legs", "Hands", "Feet"] as const;
