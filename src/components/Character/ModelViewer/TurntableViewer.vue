<template>
  <div class="relative w-full h-full min-h-0">
    <div ref="container" class="absolute inset-0"></div>

    <!-- Overlay controls -->
    <div class="absolute top-2 right-2 flex gap-2">
      <button
        class="px-2 py-1 text-xs rounded border border-white/15 bg-black/40 text-white/80 hover:bg-black/60 cursor-pointer"
        @click="autoSpin = !autoSpin">
        {{ autoSpin ? "⏸ Spin" : "▶ Spin" }}
      </button>
      <button
        class="px-2 py-1 text-xs rounded border border-white/15 bg-black/40 text-white/80 hover:bg-black/60 cursor-pointer"
        @click="resetView">
        ⟲ Reset
      </button>
    </div>

    <div
      v-if="empty"
      class="absolute inset-0 flex items-center justify-center text-sm text-text-muted pointer-events-none">
      {{ emptyMessage }}
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount, watch, computed } from "vue";
import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";
import { useModelViewerStore } from "../../../stores/modelViewerStore";
import {
  createDyeMaterial,
  type DyeMaterial,
  type DyeInit,
} from "../../../composables/useGorgonDyeMaterial";
import type { ResolvedSlot } from "../../../types/modelViewer";

const store = useModelViewerStore();
const container = ref<HTMLDivElement | null>(null);
const autoSpin = ref(true);

let renderer: THREE.WebGLRenderer | null = null;
let scene: THREE.Scene | null = null;
let camera: THREE.PerspectiveCamera | null = null;
let controls: OrbitControls | null = null;
let raf = 0;
let resizeObs: ResizeObserver | null = null;

const modelGroup = new THREE.Group();
const dyeMaterials = new Map<string, DyeMaterial>();
const gltfLoader = new GLTFLoader();
const texLoader = new THREE.TextureLoader();
let homeTarget = new THREE.Vector3();
let homeDistance = 3;

const empty = computed(() => !store.resolved?.renderable);
const emptyMessage = computed(() =>
  store.resolving
    ? "Loading model…"
    : store.resolved && !store.resolved.renderable
      ? "No 3D model available for this item"
      : "Select an item to preview",
);

function initThree() {
  const el = container.value!;
  scene = new THREE.Scene();
  scene.background = new THREE.Color(0x1a1c20);
  scene.add(modelGroup);

  camera = new THREE.PerspectiveCamera(35, el.clientWidth / el.clientHeight, 0.05, 100);
  camera.position.set(0, 1, 3);

  renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  renderer.setSize(el.clientWidth, el.clientHeight);
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.0;
  el.appendChild(renderer.domElement);

  // Soft studio environment for PBR reflections.
  const pmrem = new THREE.PMREMGenerator(renderer);
  scene.environment = pmrem.fromScene(new RoomEnvironment(), 0.04).texture;

  const key = new THREE.DirectionalLight(0xffffff, 2.0);
  key.position.set(2, 3, 2);
  scene.add(key);
  const fill = new THREE.DirectionalLight(0xbcd2ff, 0.7);
  fill.position.set(-2, 1, -1);
  scene.add(fill);
  scene.add(new THREE.HemisphereLight(0xffffff, 0x2a2a33, 0.5));

  controls = new OrbitControls(camera, renderer.domElement);
  controls.enableDamping = true;
  controls.dampingFactor = 0.08;
  controls.target.set(0, 1, 0);
  // Turntable feel: horizontal drag orbits around the vertical (waist) axis;
  // clamp the vertical angle so the model can never tumble/cartwheel over the
  // top or flip to show its underside.
  controls.minPolarAngle = Math.PI * 0.3; // ~54° — don't rise to a full top-down
  controls.maxPolarAngle = Math.PI * 0.5; // 90° — don't drop below eye level

  resizeObs = new ResizeObserver(onResize);
  resizeObs.observe(el);

  animate();
  void rebuild();
}

function onResize() {
  if (!renderer || !camera || !container.value) return;
  const el = container.value;
  camera.aspect = el.clientWidth / el.clientHeight;
  camera.updateProjectionMatrix();
  renderer.setSize(el.clientWidth, el.clientHeight);
}

function animate() {
  raf = requestAnimationFrame(animate);
  if (autoSpin.value) modelGroup.rotation.y += 0.006;
  controls?.update();
  if (renderer && scene && camera) renderer.render(scene, camera);
}

function clearModel() {
  modelGroup.rotation.set(0, 0, 0);
  for (const child of [...modelGroup.children]) {
    modelGroup.remove(child);
    child.traverse((o) => {
      const mesh = o as THREE.Mesh;
      if (mesh.geometry) mesh.geometry.dispose();
    });
  }
  for (const dm of dyeMaterials.values()) dm.dispose();
  dyeMaterials.clear();
}

function loadTexture(rel: string | undefined | null): THREE.Texture | null {
  if (!rel) return null;
  const t = texLoader.load(store.assetUrl(`textures/${rel}`));
  t.anisotropy = 8;
  t.flipY = false; // glTF/textures already in correct orientation
  return t;
}

function dyeInitFor(slot: ResolvedSlot): DyeInit {
  const c = slot.default_colors ?? {};
  const rgb = (k: string): [number, number, number] | undefined => {
    const v = c[k];
    return v ? [v[0], v[1], v[2]] : undefined;
  };
  return {
    channels: slot.dyeable ? slot.dye_channels : 0,
    color0: rgb("_Color0"),
    color1: rgb("_Color1"),
    color2: rgb("_Color2"),
    color3: rgb("_Color3"),
  };
}

/** Stand a weapon/prop upright: longest bbox axis → Y (up), thinnest → Z
 * (depth), so shields face the camera and staves/swords stand on end. */
function orientProp(mesh: THREE.Mesh) {
  mesh.geometry.computeBoundingBox();
  const size = new THREE.Vector3();
  mesh.geometry.boundingBox!.getSize(size);
  const axisVec: Record<string, THREE.Vector3> = {
    x: new THREE.Vector3(1, 0, 0),
    y: new THREE.Vector3(0, 1, 0),
    z: new THREE.Vector3(0, 0, 1),
  };
  const ordered = (["x", "y", "z"] as const)
    .map((a) => [a, size[a]] as const)
    .sort((a, b) => b[1] - a[1])
    .map(([a]) => a);
  const long = axisVec[ordered[0]];
  const mid = axisVec[ordered[1]];
  const short = axisVec[ordered[2]].clone();
  // R⁻¹ columns = [mid, long, short] ⇒ R maps long→Y, mid→X, short→Z.
  const rInv = new THREE.Matrix4().makeBasis(mid, long, short);
  if (rInv.determinant() < 0) {
    short.negate();
    rInv.makeBasis(mid, long, short);
  }
  mesh.quaternion.setFromRotationMatrix(rInv.invert());
}

async function buildSlot(slot: ResolvedSlot) {
  if (!slot.mesh_file) return;
  const gltf = await gltfLoader.loadAsync(store.assetUrl(slot.mesh_file));
  let found: THREE.Mesh | null = null;
  gltf.scene.traverse((o) => {
    if ((o as THREE.Mesh).isMesh && !found) found = o as THREE.Mesh;
  });
  if (!found) return;
  const mesh: THREE.Mesh = found;

  if (slot.is_weapon) {
    // Weapons/props have varied native orientations. Stand them up generically:
    // longest bounding-box axis → vertical (Y), thinnest → depth (Z, toward the
    // camera) so flat items like shields face front.
    orientProp(mesh);
  } else {
    // Project: Gorgon character art is authored Z-up; three.js is Y-up. Rotate
    // each body mesh so the character stands upright — then the turntable spins
    // around its true vertical (spine/waist) axis instead of tumbling.
    mesh.rotation.x = -Math.PI / 2;
  }

  const tex = slot.textures ?? {};
  const dm = createDyeMaterial(
    {
      skin: loadTexture(tex.skin),
      mask1: loadTexture(tex.mask1),
      mask2: loadTexture(tex.mask2),
      mask3: loadTexture(tex.mask3),
      normal: loadTexture(tex.normal),
    },
    dyeInitFor(slot),
  );
  mesh.material = dm.material;
  dyeMaterials.set(slot.slot, dm);
  modelGroup.add(mesh);
}

async function rebuild() {
  if (!scene) return;
  clearModel();
  const res = store.resolved;
  if (!res || !res.renderable) return;

  for (const slot of res.slots) {
    if (slot.mesh_file) {
      try {
        await buildSlot(slot);
      } catch (e) {
        console.warn(`Failed to load slot ${slot.slot}:`, e);
      }
    }
  }
  frameCamera();
}

function frameCamera() {
  if (!camera || !controls || modelGroup.children.length === 0) return;
  // modelGroup.rotation is reset to 0 by clearModel(), so local == world here.
  const box = new THREE.Box3().setFromObject(modelGroup);
  if (box.isEmpty()) return;
  const size = box.getSize(new THREE.Vector3());
  const center = box.getCenter(new THREE.Vector3());
  // Recenter meshes on the group origin so the turntable spins in place.
  for (const child of modelGroup.children) child.position.sub(center);
  const maxDim = Math.max(size.x, size.y, size.z);
  const fov = (camera.fov * Math.PI) / 180;
  homeDistance = (maxDim / 2 / Math.tan(fov / 2)) * 1.5;
  homeTarget = new THREE.Vector3(0, 0, 0);
  resetView();
}

function resetView() {
  if (!camera || !controls) return;
  controls.target.copy(homeTarget);
  camera.position.set(
    homeTarget.x,
    homeTarget.y + homeDistance * 0.15,
    homeTarget.z + homeDistance,
  );
  camera.updateProjectionMatrix();
  controls.update();
}

/** Called by parent to recolor a dye channel of a slot live. */
function setDye(slot: string, channel: number, hex: string) {
  dyeMaterials.get(slot)?.setChannelColor(channel, hex);
}

defineExpose({ setDye });

watch(
  () => store.resolved,
  () => void rebuild(),
);

onMounted(initThree);
onBeforeUnmount(() => {
  cancelAnimationFrame(raf);
  resizeObs?.disconnect();
  clearModel();
  controls?.dispose();
  renderer?.dispose();
  renderer?.domElement.remove();
});
</script>
