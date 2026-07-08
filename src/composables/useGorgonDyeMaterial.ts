/**
 * Replicates Project: Gorgon's `Gorgon/Character` dye shader as a three.js
 * material, so armor recolors live as the user changes dye channels.
 *
 * The game shader composites up to three dye channels over the skin base:
 *   base = skin(_MainTex).rgb * _Color0
 *   for each channel i in 1..3:
 *     m = _MaskMapᵢ   // rgb = grayscale detail, a = coverage
 *     base = mix(base, _Colorᵢ * m.rgb, m.a * _ColorᵢOpacity)
 *
 * We drive a MeshStandardMaterial (for real PBR lighting + normal mapping) and
 * override only the albedo via onBeforeCompile. Changing a dye color just
 * mutates a uniform — no rebuild.
 */
import * as THREE from "three";

export interface DyeTextures {
  skin?: THREE.Texture | null;
  mask1?: THREE.Texture | null;
  mask2?: THREE.Texture | null;
  mask3?: THREE.Texture | null;
  normal?: THREE.Texture | null;
}

export interface DyeInit {
  channels: number; // 0..3
  color0?: [number, number, number];
  color1?: [number, number, number];
  color2?: [number, number, number];
  color3?: [number, number, number];
  opacity1?: number;
  opacity2?: number;
  opacity3?: number;
}

export interface DyeMaterial {
  material: THREE.MeshStandardMaterial;
  channels: number;
  /** Set a dye channel (1..3) color from an #rrggbb hex string. */
  setChannelColor(channel: number, hex: string): void;
  /** Current channel colors as hex, for binding UI swatches. */
  getChannelHex(channel: number): string;
  dispose(): void;
}

function toColor(rgb?: [number, number, number], fallback = 0xffffff): THREE.Color {
  if (!rgb) return new THREE.Color(fallback);
  // Material colors in the catalog are already linear-ish shader values; treat
  // as sRGB tints for intuitive picking.
  return new THREE.Color().setRGB(rgb[0], rgb[1], rgb[2], THREE.SRGBColorSpace);
}

// Shared 1×1 white texture used as a `map` fallback so `vMapUv` is always set
// up (see note in createDyeMaterial). Module-level singleton — never disposed.
let _whiteTex: THREE.Texture | null = null;
function whiteFallback(): THREE.Texture {
  if (!_whiteTex) {
    _whiteTex = new THREE.DataTexture(
      new Uint8Array([255, 255, 255, 255]),
      1,
      1,
      THREE.RGBAFormat,
    );
    _whiteTex.colorSpace = THREE.SRGBColorSpace;
    _whiteTex.needsUpdate = true;
  }
  return _whiteTex;
}

export function createDyeMaterial(tex: DyeTextures, init: DyeInit): DyeMaterial {
  if (tex.skin) tex.skin.colorSpace = THREE.SRGBColorSpace;
  for (const m of [tex.mask1, tex.mask2, tex.mask3]) {
    if (m) m.colorSpace = THREE.SRGBColorSpace;
  }
  if (tex.normal) tex.normal.colorSpace = THREE.NoColorSpace;

  const material = new THREE.MeshStandardMaterial({
    // A `map` MUST be present or three.js never defines the `vMapUv` varying our
    // dye shader samples — which silently breaks fully-covered gear (helmets,
    // full plate) whose material has no skin `_MainTex`. Fall back to 1×1 white.
    map: tex.skin ?? whiteFallback(),
    normalMap: tex.normal ?? null,
    metalness: 0.15,
    roughness: 0.62,
    color: 0xffffff,
  });

  const uniforms = {
    uColor0: { value: toColor(init.color0, 0xffffff) },
    uColor1: { value: toColor(init.color1, 0x808080) },
    uColor2: { value: toColor(init.color2, 0x808080) },
    uColor3: { value: toColor(init.color3, 0x808080) },
    uOpacity: {
      value: new THREE.Vector3(
        init.opacity1 ?? 1,
        init.opacity2 ?? 1,
        init.opacity3 ?? 1,
      ),
    },
    uChannels: { value: init.channels },
    uMask1: { value: tex.mask1 ?? null },
    uMask2: { value: tex.mask2 ?? null },
    uMask3: { value: tex.mask3 ?? null },
  };

  material.onBeforeCompile = (shader) => {
    Object.assign(shader.uniforms, uniforms);
    shader.fragmentShader =
      `
      uniform vec3 uColor0;
      uniform vec3 uColor1;
      uniform vec3 uColor2;
      uniform vec3 uColor3;
      uniform vec3 uOpacity;
      uniform float uChannels;
      uniform sampler2D uMask1;
      uniform sampler2D uMask2;
      uniform sampler2D uMask3;
      ` + shader.fragmentShader;

    // After <map_fragment>, diffuseColor.rgb == skin (map) * material.color.
    shader.fragmentShader = shader.fragmentShader.replace(
      "#include <map_fragment>",
      `
      #include <map_fragment>
      {
        vec3 dyeBase = diffuseColor.rgb * uColor0;
        vec3 dyeCol = dyeBase;
        if (uChannels > 0.5) { vec4 m = texture2D(uMask1, vMapUv); dyeCol = mix(dyeCol, uColor1 * m.rgb, m.a * uOpacity.x); }
        if (uChannels > 1.5) { vec4 m = texture2D(uMask2, vMapUv); dyeCol = mix(dyeCol, uColor2 * m.rgb, m.a * uOpacity.y); }
        if (uChannels > 2.5) { vec4 m = texture2D(uMask3, vMapUv); dyeCol = mix(dyeCol, uColor3 * m.rgb, m.a * uOpacity.z); }
        diffuseColor.rgb = dyeCol;
      }
      `,
    );
    material.userData.shader = shader;
  };

  const channelUniform = (c: number) =>
    c === 1 ? uniforms.uColor1 : c === 2 ? uniforms.uColor2 : uniforms.uColor3;

  return {
    material,
    channels: init.channels,
    setChannelColor(channel: number, hex: string) {
      if (channel < 1 || channel > 3) return;
      channelUniform(channel).value.set(hex);
    },
    getChannelHex(channel: number) {
      return "#" + channelUniform(channel).value.getHexString(THREE.SRGBColorSpace);
    },
    dispose() {
      material.dispose();
      for (const t of [tex.skin, tex.mask1, tex.mask2, tex.mask3, tex.normal]) {
        t?.dispose();
      }
    },
  };
}
