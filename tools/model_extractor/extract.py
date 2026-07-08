#!/usr/bin/env python3
"""Project: Gorgon 3D asset extractor for glogger's Model Viewer.

Reads the game's local Unity Addressable bundles and emits a compact,
web-renderable cache:

  <out>/
    catalog.json                 index of appearances / materials / weapons
    meshes/<key>.glb             geometry-only glTF (binary), one per mesh
    textures/<name>.png          deduped textures (skin, dye masks, normal, ...)

The catalog is consumed by the Tauri backend (`model_assets.rs`) and the
frontend three.js viewer. Nothing here ships game art in the repo — it runs
against the *user's* install and writes to their local app cache.

Usage:
  python extract.py --game-dir "<...>/WindowsPlayer_Data/StreamingAssets/aa/StandaloneWindows64" --out <cache-dir>
  python extract.py --game-dir <...> --out <...> --only "eq-m2-chest" --skip-textures   # fast subset

Dependencies: UnityPy, pygltflib, numpy, Pillow  (see requirements.txt)
"""
from __future__ import annotations
import argparse, glob, hashlib, json, os, re, sys, time
import numpy as np

CATALOG_SCHEMA = 2

# ---- mesh source bundles ---------------------------------------------------
GEAR_BUNDLE_GLOB = "*newmodel_assets_all*"      # modern m2/f2 player gear + base bodies + skeleton
WEAPON_BUNDLE_GLOBS = [                          # static-mesh weapons / off-hands
    "*eq-x-*", "*eq-orb-*", "*myfg-weapon-pack*", "*vendortable_weapons*",
]

SLOT_FROM_TOKEN = {
    "chest": "Chest", "body": "Chest", "feet": "Feet", "hands": "Hands",
    "head": "Head", "legs": "Legs", "hair": "Hair", "beard": "Beard",
    "eyebrows": "Eyebrows", "skin": "Skin",
}


def log(msg: str):
    print(msg, flush=True)


# ---- glb writer ------------------------------------------------------------
def _parse_obj(obj_txt: str):
    """Parse UnityPy Mesh.export() OBJ text into flat (pos, nrm, uv, tris)
    arrays. UnityPy emits unified vertices (v/vt/vn indices match per corner)
    and handles Unity's winding/handedness, so we read arrays directly and only
    flip V for glTF's top-left UV origin."""
    V, VT, VN, F = [], [], [], []
    for ln in obj_txt.splitlines():
        if ln.startswith("v "):
            V.append([float(x) for x in ln[2:].split()])
        elif ln.startswith("vt "):
            VT.append([float(x) for x in ln[3:].split()[:2]])
        elif ln.startswith("vn "):
            VN.append([float(x) for x in ln[3:].split()])
        elif ln.startswith("f "):
            F.append([int(c.split("/")[0]) - 1 for c in ln[2:].split()])
    if not V or not F:
        return None
    pos = np.array(V, dtype=np.float32)
    nrm = np.array(VN, dtype=np.float32) if VN else np.zeros_like(pos)
    uv = np.array(VT, dtype=np.float32) if VT else np.zeros((len(V), 2), np.float32)
    if len(uv):
        uv[:, 1] = 1.0 - uv[:, 1]
    tris = []
    for f in F:
        for i in range(1, len(f) - 1):
            tris += [f[0], f[i], f[i + 1]]
    return pos, nrm, uv, np.array(tris, dtype=np.uint32)


def obj_to_glb(obj_txt, out_path: str) -> dict:
    """Write one or more UnityPy OBJ meshes into a single geometry-only .glb.

    `obj_txt` may be a single OBJ string or a list of them (multi-part models
    like a lute body + strings are merged into one geometry buffer)."""
    from pygltflib import (GLTF2, Scene, Node, Mesh as GMesh, Primitive, Attributes,
                           Accessor, BufferView, Buffer, ARRAY_BUFFER,
                           ELEMENT_ARRAY_BUFFER, FLOAT, UNSIGNED_INT, SCALAR, VEC2, VEC3)
    parts = [obj_txt] if isinstance(obj_txt, str) else list(obj_txt)
    pos_all, nrm_all, uv_all, tri_all = [], [], [], []
    base = 0
    for txt in parts:
        parsed = _parse_obj(txt)
        if not parsed:
            continue
        p, n, u, t = parsed
        pos_all.append(p); nrm_all.append(n); uv_all.append(u)
        tri_all.append(t + base)
        base += len(p)
    if not pos_all:
        return {}
    pos = np.concatenate(pos_all)
    nrm = np.concatenate(nrm_all)
    uv = np.concatenate(uv_all)
    tris = np.concatenate(tri_all).astype(np.uint32)

    blob = b""
    def add(a: np.ndarray):
        nonlocal blob
        off = len(blob)
        b = a.tobytes()
        blob += b + b"\x00" * ((4 - len(b) % 4) % 4)
        return off, len(b)
    o_i, l_i = add(tris)
    o_p, l_p = add(pos)
    o_n, l_n = add(nrm)
    o_u, l_u = add(uv)

    g = GLTF2()
    g.scenes = [Scene(nodes=[0])]; g.scene = 0
    g.nodes = [Node(mesh=0)]
    g.meshes = [GMesh(primitives=[Primitive(
        attributes=Attributes(POSITION=1, NORMAL=2, TEXCOORD_0=3), indices=0)])]
    g.buffers = [Buffer(byteLength=len(blob))]
    g.bufferViews = [
        BufferView(buffer=0, byteOffset=o_i, byteLength=l_i, target=ELEMENT_ARRAY_BUFFER),
        BufferView(buffer=0, byteOffset=o_p, byteLength=l_p, target=ARRAY_BUFFER),
        BufferView(buffer=0, byteOffset=o_n, byteLength=l_n, target=ARRAY_BUFFER),
        BufferView(buffer=0, byteOffset=o_u, byteLength=l_u, target=ARRAY_BUFFER),
    ]
    g.accessors = [
        Accessor(bufferView=0, componentType=UNSIGNED_INT, count=len(tris), type=SCALAR),
        Accessor(bufferView=1, componentType=FLOAT, count=len(pos), type=VEC3,
                 min=pos.min(0).tolist(), max=pos.max(0).tolist()),
        Accessor(bufferView=2, componentType=FLOAT, count=len(nrm), type=VEC3),
        Accessor(bufferView=3, componentType=FLOAT, count=len(uv), type=VEC2),
    ]
    g.set_binary_blob(blob)
    g.save_binary(out_path)
    return {"verts": int(len(pos)), "tris": int(len(tris) // 3)}


# ---- material / texture extraction -----------------------------------------
# Maps shader texture-property names → our catalog labels. Includes both the
# Gorgon/Character dye-shader names (_MaskMapN/_NormalMap/_MetallicMap) and the
# Unity Standard-shader names used by weapons/old gear (_BumpMap/_MetallicGlossMap).
TEX_SLOTS = {
    "_MainTex": "skin", "_MaskMap1": "mask1", "_MaskMap2": "mask2",
    "_MaskMap3": "mask3", "_NormalMap": "normal", "_MetallicMap": "metallic",
    "_DetailMap": "detail", "_EmissionMap": "emission",
    "_BumpMap": "normal", "_MetallicGlossMap": "metallic",
}


def safe_name(s: str) -> str:
    return re.sub(r"[^A-Za-z0-9._-]", "_", s)


def extract_material(mat, tex_dir: str, seen_tex: dict, skip_textures: bool) -> dict:
    sp = mat.m_SavedProperties
    texenvs = {n: te for n, te in sp.m_TexEnvs}
    out_tex = {}
    for prop, label in TEX_SLOTS.items():
        te = texenvs.get(prop)
        if not te or not te.m_Texture or te.m_Texture.m_PathID == 0:
            continue
        try:
            tex = te.m_Texture.read()
        except Exception:
            continue
        fname = safe_name(tex.m_Name) + ".png"
        out_tex[label] = fname
        key = fname
        if not skip_textures and key not in seen_tex:
            try:
                tex.image.save(os.path.join(tex_dir, fname))
                seen_tex[key] = True
            except Exception as e:
                log(f"    ! texture {tex.m_Name}: {e}")
    dye_channels = sum(1 for k in ("mask1", "mask2", "mask3") if k in out_tex)
    colors = {n: [round(c.r, 4), round(c.g, 4), round(c.b, 4), round(c.a, 4)]
              for n, c in sp.m_Colors if n in ("_Color", "_Color0", "_Color1", "_Color2", "_Color3", "_EmissionColors")}
    # Standard shader uses `_Color` for the base tint; map it to `_Color0` so the
    # viewer (which reads _Color0) tints weapons/old gear correctly.
    if "_Color0" not in colors and "_Color" in colors:
        colors["_Color0"] = colors["_Color"]
    floats = {n: round(v, 4) for n, v in sp.m_Floats
              if n in ("_Color1Opacity", "_Color2Opacity", "_Color3Opacity",
                       "_Metallic", "_Smoothness", "_Occlusion", "_EmissionStrength")}
    return {"textures": out_tex, "dyeChannels": dye_channels, "colors": colors, "floats": floats}


# ---- bundle walkers --------------------------------------------------------
def renderer_of(go):
    for cp in go.m_Component:
        try:
            c = cp.component.read()
        except Exception:
            continue
        tn = c.object_reader.type.name
        if tn in ("SkinnedMeshRenderer", "MeshRenderer"):
            return c, tn
    return None, None


def mesh_of(renderer, tn, by_id):
    try:
        if tn == "SkinnedMeshRenderer":
            return renderer.m_Mesh.read()
        # MeshRenderer: find sibling MeshFilter on same GameObject
        go = renderer.m_GameObject.read()
        for cp in go.m_Component:
            c = cp.component.read()
            if c.object_reader.type.name == "MeshFilter":
                return c.m_Mesh.read()
    except Exception:
        return None
    return None


def derive_sex_slot(go_name: str):
    m = re.match(r"^eq-(m|f|x)2?-(?:m2-|f2-|x2-)?([a-z]+)", go_name)
    sex = None; slot = None
    mm = re.match(r"^eq-(m|f|x)2?-([a-z]+)", go_name)
    if mm:
        sex = {"m": "m", "f": "f", "x": "x"}.get(mm.group(1))
        slot = SLOT_FROM_TOKEN.get(mm.group(2))
    return sex, slot


def derive_key(filename: str):
    """Addressable key from an individual-asset bundle filename, e.g.
    `defaultlocalgroup_assets_eq-x-flask1_<hash>.bundle` → `eq-x-flask1`."""
    m = re.match(r"^defaultlocalgroup_assets_(.+)_[0-9a-f]{32}\.bundle$", filename)
    return m.group(1) if m else None


def slot_for_key(key: str):
    """Equipment slot for a gear key (searching hyphen segments), or None if the
    key isn't gear (e.g. a weapon)."""
    for tok in key.split("-"):
        if tok in SLOT_FROM_TOKEN:
            return SLOT_FROM_TOKEN[tok]
    return None


def sex_for_key(key: str):
    m = re.match(r"^eq-(m|f)2?-", key)
    return m.group(1) if m else None


# GameObjects whose renderers are alternates/attachments, not the held model.
_SKIP_RENDERER_NAMES = ("sheath", "override", "mountpoint", "particle")


def extract_gear(bundle_path, out, seen_tex, only_re, skip_textures):
    import UnityPy
    log(f"[gear] loading {os.path.basename(bundle_path)} ...")
    env = UnityPy.load(bundle_path)
    objs = list(env.objects)
    appearances, materials = {}, {}
    mesh_dir = os.path.join(out, "meshes"); tex_dir = os.path.join(out, "textures")

    # index all materials by name (dye info) — used via ^Armor= overrides
    for o in objs:
        if o.type.name != "Material":
            continue
        m = o.read()
        if only_re and not only_re.search(m.m_Name):
            continue
        if m.m_Name in materials:
            continue
        materials[m.m_Name] = extract_material(m, tex_dir, seen_tex, skip_textures)

    # index meshes keyed by GameObject name (the EquipAppearance mesh key)
    exported = 0
    for o in objs:
        if o.type.name != "GameObject":
            continue
        go = o.read()
        name = go.m_Name
        if not name.startswith(("eq-", "m2-", "f2-")):
            continue
        if only_re and not only_re.search(name):
            continue
        renderer, tn = renderer_of(go)
        if not renderer:
            continue
        mesh = mesh_of(renderer, tn, None)
        if not mesh:
            continue
        glb_rel = f"meshes/{safe_name(name)}.glb"
        glb_abs = os.path.join(out, glb_rel.replace("/", os.sep))
        if not os.path.exists(glb_abs):
            try:
                stats = obj_to_glb(mesh.export(), glb_abs)
            except Exception as e:
                log(f"    ! mesh {name}: {e}"); continue
            if not stats:
                continue
            exported += 1
        # default material (fallback when item has no ^Armor= override)
        default_mat = None
        try:
            if renderer.m_Materials and renderer.m_Materials[0].m_PathID != 0:
                default_mat = renderer.m_Materials[0].read().m_Name
        except Exception:
            pass
        sex, slot = derive_sex_slot(name)
        appearances[name] = {"meshFile": glb_rel, "sex": sex, "slot": slot,
                             "defaultMaterial": default_mat, "skinned": tn == "SkinnedMeshRenderer"}
    log(f"[gear] {exported} meshes, {len(materials)} materials")
    return appearances, materials


def _renderer_go_name(r):
    try:
        return r.m_GameObject.read().m_Name
    except Exception:
        return ""


def extract_individual_bundles(game_dir, out, seen_tex, only_re, skip_textures):
    """Extract each individual `defaultlocalgroup_assets_<key>_<hash>.bundle` as
    one merged model keyed by its **addressable key** (the name EquipAppearance
    references, e.g. `eq-x-flask1`, `eq-w-plate-chest-1`) — not by the internal
    child-mesh name. Weapons (eq-x-*/eq-orb-*) go in `weapons`; keys with a slot
    token (chest/legs/…) go in `appearances`. All in-bundle materials are
    indexed so `^Armor=` overrides resolve."""
    import UnityPy
    weapons, appearances, materials = {}, {}, {}
    tex_dir = os.path.join(out, "textures")

    paths = set(glob.glob(os.path.join(game_dir, "defaultlocalgroup_assets_eq-*")))
    n_bundles = 0
    for bp in sorted(paths):
        key = derive_key(os.path.basename(bp))
        if not key:
            continue
        if only_re and not only_re.search(key):
            continue
        try:
            env = UnityPy.load(bp)
        except Exception:
            continue
        objs = list(env.objects)
        n_bundles += 1

        # All in-bundle materials (for ^Armor= resolution).
        for o in objs:
            if o.type.name != "Material":
                continue
            try:
                m = o.read()
                materials.setdefault(m.m_Name, extract_material(m, tex_dir, seen_tex, skip_textures))
            except Exception:
                continue

        # Collect renderer meshes (merged into one model), skipping sheath/mount
        # alternates. Track the primary (first real) material.
        part_objs, primary_mat = [], None
        for o in objs:
            if o.type.name not in ("MeshRenderer", "SkinnedMeshRenderer"):
                continue
            try:
                r = o.read()
            except Exception:
                continue
            gname = _renderer_go_name(r).lower()
            if any(s in gname for s in _SKIP_RENDERER_NAMES):
                continue
            mesh = mesh_of(r, o.type.name, None)
            if not mesh:
                continue
            try:
                part_objs.append(mesh.export())
            except Exception:
                continue
            if primary_mat is None:
                try:
                    if r.m_Materials and r.m_Materials[0].m_PathID != 0:
                        primary_mat = r.m_Materials[0].read().m_Name
                except Exception:
                    pass
        if not part_objs:
            continue

        glb_rel = f"meshes/{safe_name(key)}.glb"
        glb_abs = os.path.join(out, glb_rel.replace("/", os.sep))
        if not os.path.exists(glb_abs):
            try:
                if not obj_to_glb(part_objs, glb_abs):
                    continue
            except Exception as e:
                log(f"    ! {key}: {e}"); continue

        slot = slot_for_key(key)
        if slot:
            appearances[key] = {"meshFile": glb_rel, "sex": sex_for_key(key), "slot": slot,
                                "defaultMaterial": primary_mat, "skinned": True}
        else:
            weapons[key] = {"meshFile": glb_rel, "material": primary_mat}
    log(f"[individual] {len(weapons)} weapons + {len(appearances)} gear from "
        f"{n_bundles} bundles, {len(materials)} materials")
    return weapons, appearances, materials


def _go_transform(go):
    for cp in go.m_Component:
        try:
            c = cp.component.read()
            if c.object_reader.type.name in ("Transform", "RectTransform"):
                return c
        except Exception:
            pass
    return None


def _subtree_meshes(go):
    """Merged OBJ parts + primary material for a GameObject and all its
    descendants (weapon-pack entries are containers with the mesh on children)."""
    parts, primary_mat = [], None
    root_t = _go_transform(go)
    stack = [root_t] if root_t else []
    gos = [go]
    while stack:
        t = stack.pop()
        for ch in getattr(t, "m_Children", []):
            try:
                ct = ch.read()
                gos.append(ct.m_GameObject.read())
                stack.append(ct)
            except Exception:
                pass
    for g in gos:
        if any(s in g.m_Name.lower() for s in _SKIP_RENDERER_NAMES):
            continue
        r, tn = renderer_of(g)
        if not r:
            continue
        mesh = mesh_of(r, tn, None)
        if not mesh:
            continue
        try:
            parts.append(mesh.export())
        except Exception:
            continue
        if primary_mat is None:
            try:
                if r.m_Materials and r.m_Materials[0].m_PathID != 0:
                    primary_mat = r.m_Materials[0].read().m_Name
            except Exception:
                pass
    return parts, primary_mat


def extract_packs(game_dir, out, seen_tex, only_re, skip_textures):
    """Extract weapons from shared packs (e.g. myfg-weapon-pack) where each
    weapon is an `eq-x-*` GameObject containing its meshes as children — keyed
    by that GameObject name (the EquipAppearance key)."""
    import UnityPy
    weapons, materials = {}, {}
    tex_dir = os.path.join(out, "textures")
    n_bundles = 0
    for pat in ["*myfg-weapon-pack*", "*vendortable_weapons*", "*vendortable_bows*"]:
        for bp in glob.glob(os.path.join(game_dir, pat)):
            try:
                env = UnityPy.load(bp)
            except Exception:
                continue
            n_bundles += 1
            objs = list(env.objects)
            for o in objs:
                if o.type.name == "Material":
                    try:
                        m = o.read()
                        materials.setdefault(m.m_Name, extract_material(m, tex_dir, seen_tex, skip_textures))
                    except Exception:
                        pass
            for o in objs:
                if o.type.name != "GameObject":
                    continue
                go = o.read()
                name = go.m_Name
                if not name.startswith("eq-"):
                    continue
                if only_re and not only_re.search(name):
                    continue
                parts, pmat = _subtree_meshes(go)
                if not parts:
                    continue
                glb_rel = f"meshes/{safe_name(name)}.glb"
                glb_abs = os.path.join(out, glb_rel.replace("/", os.sep))
                if not os.path.exists(glb_abs):
                    try:
                        if not obj_to_glb(parts, glb_abs):
                            continue
                    except Exception as e:
                        log(f"    ! {name}: {e}"); continue
                weapons[name] = {"meshFile": glb_rel, "material": pmat}
    log(f"[packs] {len(weapons)} weapons from {n_bundles} pack bundles, {len(materials)} materials")
    return weapons, materials


def extract_extra_materials(game_dir, out, seen_tex, skip_textures, needed=None):
    """Scan material-bearing bundles (plate/armor/body/mat-*) for named dye
    materials referenced by gear `^Armor=` (e.g. WerewolfPlate1, CowArmor1) that
    live outside the mesh bundle."""
    import UnityPy
    materials = {}
    tex_dir = os.path.join(out, "textures")
    globs = ["defaultlocalgroup_assets_*plate*", "defaultlocalgroup_assets_*armor*",
             "*mat-*"]
    paths = set()
    for g in globs:
        paths |= set(glob.glob(os.path.join(game_dir, g)))
    n = 0
    for bp in sorted(paths):
        try:
            env = UnityPy.load(bp)
        except Exception:
            continue
        n += 1
        for o in env.objects:
            if o.type.name != "Material":
                continue
            try:
                m = o.read()
                if m.m_Name in materials:
                    continue
                materials[m.m_Name] = extract_material(m, tex_dir, seen_tex, skip_textures)
            except Exception:
                continue
    log(f"[materials] {len(materials)} materials from {n} material bundles")
    return materials


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--game-dir", required=True, help="StandaloneWindows64 dir")
    ap.add_argument("--out", required=True, help="output cache dir")
    ap.add_argument("--only", default=None, help="regex filter on object names")
    ap.add_argument("--skip-textures", action="store_true")
    ap.add_argument("--no-weapons", action="store_true",
                    help="skip individual eq-* bundles (weapons + separate gear)")
    ap.add_argument("--skip-extra-materials", action="store_true",
                    help="skip the plate/armor/mat-* material scan")
    args = ap.parse_args()

    only_re = re.compile(args.only, re.I) if args.only else None
    os.makedirs(os.path.join(args.out, "meshes"), exist_ok=True)
    os.makedirs(os.path.join(args.out, "textures"), exist_ok=True)
    seen_tex: dict = {}
    t0 = time.time()

    gear_bundles = glob.glob(os.path.join(args.game_dir, GEAR_BUNDLE_GLOB))
    if not gear_bundles:
        log(f"FATAL: no gear bundle matching {GEAR_BUNDLE_GLOB} in {args.game_dir}")
        sys.exit(2)
    gear_bundle = gear_bundles[0]
    src_hash = hashlib.sha1(os.path.basename(gear_bundle).encode()).hexdigest()[:12]

    appearances, materials = extract_gear(gear_bundle, args.out, seen_tex, only_re, args.skip_textures)
    weapons = {}
    if not args.no_weapons:
        w, ind_app, ind_mats = extract_individual_bundles(
            args.game_dir, args.out, seen_tex, only_re, args.skip_textures)
        weapons.update(w)
        # newmodel (extract_gear) takes priority; individual bundles fill gaps.
        for k, v in ind_app.items():
            appearances.setdefault(k, v)
        for k, v in ind_mats.items():
            materials.setdefault(k, v)
        # Shared weapon packs (the bulk of weapons live here).
        pw, pmats = extract_packs(args.game_dir, args.out, seen_tex, only_re, args.skip_textures)
        for k, v in pw.items():
            weapons.setdefault(k, v)
        for k, v in pmats.items():
            materials.setdefault(k, v)
        if not args.skip_extra_materials:
            for k, v in extract_extra_materials(
                    args.game_dir, args.out, seen_tex, args.skip_textures).items():
                materials.setdefault(k, v)

    catalog = {
        "schema": CATALOG_SCHEMA,
        "sourceBundle": os.path.basename(gear_bundle),
        "sourceHash": src_hash,
        "generatedAt": int(time.time()),
        "appearances": appearances,
        "materials": materials,
        "weapons": weapons,
    }
    with open(os.path.join(args.out, "catalog.json"), "w", encoding="utf-8") as f:
        json.dump(catalog, f, indent=1)
    log(f"DONE in {time.time()-t0:.1f}s  appearances={len(appearances)} "
        f"materials={len(materials)} weapons={len(weapons)} textures={len(seen_tex)}")


if __name__ == "__main__":
    main()
