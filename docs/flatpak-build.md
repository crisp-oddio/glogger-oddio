# Flatpak Build & Run (Linux)

How glogger is packaged as a Flatpak, how to build it locally, and the fixes that
got the bundle rendering correctly on a fresh Debian-family install.

The manifest lives at [`flatpak/io.github.crisp_oddio.glogger.yml`](../flatpak/io.github.crisp_oddio.glogger.yml).
CI builds and attaches the bundle to releases via
[`.github/workflows/flatpak.yml`](../.github/workflows/flatpak.yml) on every `v*` tag.

## Overview

The app is built from source inside the Flatpak sandbox:

- **Runtime / SDK:** `org.gnome.Platform` / `org.gnome.Sdk` `47`
- **SDK extensions:** `rust-stable`, `node22`
- **Frontend:** Vue built with `npm run build` → `dist/`
- **Backend:** Tauri/Rust binary built with `cargo build --release`
- Assets are embedded and served from `tauri://localhost` (no runtime web server)

Because the sandbox has no network access during the build, all npm and Cargo
dependencies are pre-fetched into offline source manifests:

- `flatpak/generated-cargo-sources.json` (from `flatpak-cargo-generator`)
- `flatpak/generated-node-sources.json` (from `flatpak-node-generator`)

These are **build artifacts** — regenerated fresh by CI and ignored by git. Don't
commit them; they go stale against `Cargo.lock` / `package-lock.json`.

## Building locally

Prerequisites (Debian/Ubuntu example):

```bash
sudo apt-get install -y flatpak flatpak-builder python3-pip pipx
```

Install the offline-source generators. **Use the upstream git versions**, not the
PyPI releases — the PyPI `flatpak-node-generator` (0.1.1) cannot parse the nested
`node_modules` that newer Tailwind pulls in and fails with
`NotImplementedError: Don't know how to handle package .../@emnapi/core`:

```bash
pipx install "git+https://github.com/flatpak/flatpak-builder-tools.git#subdirectory=node"
pipx install flatpak-cargo-generator
```

Add Flathub and install the runtime + SDKs:

```bash
flatpak remote-add --if-not-exists --user flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user -y flathub \
  org.gnome.Platform//47 org.gnome.Sdk//47 \
  org.freedesktop.Sdk.Extension.rust-stable//24.08 \
  org.freedesktop.Sdk.Extension.node22//24.08
```

Generate the offline caches and build:

```bash
flatpak-node-generator npm package-lock.json -o flatpak/generated-node-sources.json
flatpak-cargo-generator src-tauri/Cargo.lock  -o flatpak/generated-cargo-sources.json

flatpak-builder --user --install --force-clean build-dir \
  flatpak/io.github.crisp_oddio.glogger.yml
```

Run it:

```bash
flatpak run io.github.crisp_oddio.glogger
```

## Fixes that got it rendering

Three manifest bugs prevented a working bundle. All three also affected CI (which
uses the same generators), so they're fixed in the manifest itself:

1. **npm cache path.** `flatpak-node-generator` unpacks the offline npm cache to
   `flatpak-node/npm-cache`, but `npm_config_cache` pointed at a bare
   `npm-cache` dir. `npm ci --offline` then failed with
   `ENOTCACHED ... cache mode is 'only-if-cached'`. Fixed by pointing
   `npm_config_cache` at `.../flatpak-node/npm-cache`.

2. **Missing Node header setup.** The generated sources ship a
   `flatpak-node/setup_sdk_node_headers.sh` that links the Node SDK headers into
   the cache for native module builds. It must run before `npm ci`; added as the
   first build command.

3. **Blank white window (the big one).** The production build was baking in the
   dev-server URL (`http://localhost:1420`) as the webview's startup URL, so the
   sandboxed WebKit tried to connect to a server that wasn't running and rendered
   a blank page (`Could not connect to localhost: Connection refused`). Tauri's
   build step falls back to `config.build.devUrl` when `TAURI_DEV_URL` isn't set.
   Fixed by setting `"build":{"devUrl":null}` in the `TAURI_CONFIG` env override so
   the binary embeds `frontendDist` and serves from `tauri://localhost`.

## Runtime notes

On this NVIDIA + X11 machine the bundle logs some non-fatal noise that does **not**
prevent the app from working:

- `Failed to load module "canberra-gtk-module"` — sound module not in the sandbox.
- `GBM-DRV error` / `KMS: DRM_IOCTL_MODE_CREATE_DUMB failed: Permission denied` —
  WebKit GPU-compositing quirk under the NVIDIA driver inside the sandbox.

If the window is ever blank on a GPU where compositing misbehaves, launching with
`--env=WEBKIT_DISABLE_DMABUF_RENDERER=1` (optionally `LIBGL_ALWAYS_SOFTWARE=1`)
forces a software path and clears the GBM/DRM errors. The app itself renders fine
without these once fix #3 is in place.

## Verified

Built and ran successfully as a Flatpak on Debian 13 (x11, NVIDIA 550):

```
glogger v0.9.1 starting up
Database initialized
Game data ready: v470 — 10730 items, 182 skills, 4427 recipes, 338 npcs, ...
First-time setup — entering setup wizard
```

The setup wizard rendering confirms the Vue frontend loads from the embedded
assets — not just the Rust backend booting.
