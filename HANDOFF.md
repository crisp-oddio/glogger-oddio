# glogger — Session Handoff

**Date:** 2026-06-18
**Machine:** Debian 13, X11, NVIDIA 550 (Linux dev box)
**Outcome:** glogger builds and runs as a Linux Flatpak; **v0.9.2 released** with the bundle attached.

---

## TL;DR

- Built the Flatpak from source on Linux, found and fixed why it showed a blank white window.
- Bumped version **0.9.1 → 0.9.2**, wrote release notes, documented the build in `docs/flatpak-build.md`.
- Fixed the GitHub Actions Flatpak workflow (it was broken in 4 separate ways).
- Published [v0.9.2](https://github.com/crisp-oddio/glogger-oddio/releases/tag/v0.9.2) with `glogger.flatpak` attached.

---

## The real bug: blank white window

The production Tauri binary had the **dev-server URL (`http://localhost:1420`) baked in** as
the webview startup URL. Sandboxed WebKit tried to connect to a server that wasn't running and
rendered a blank page (`Could not connect to localhost: Connection refused`). Tauri's build step
falls back to `config.build.devUrl` when `TAURI_DEV_URL` isn't set.

**Fix:** set `"build":{"devUrl":null}` in the manifest's `TAURI_CONFIG` env override so the binary
embeds `frontendDist` and serves from `tauri://localhost`.

Two earlier offline-build fixes in the same manifest:
1. `npm_config_cache` must point at `.../flatpak-node/npm-cache` (where flatpak-node-generator
   unpacks), or `npm ci --offline` fails with `ENOTCACHED`.
2. Run `flatpak-node/setup_sdk_node_headers.sh` before `npm ci` to link Node SDK headers.

Full detail: [`docs/flatpak-build.md`](docs/flatpak-build.md).

## CI workflow fixes (`.github/workflows/flatpak.yml`)

The Flatpak workflow was broken against current upstream — fixed in four commits:
1. Cargo generator now needs `tomlkit` (was `ModuleNotFoundError`).
2. The node generator's single-file URL 404s upstream → install as a pip package from git.
3. `flatpak-builder` isn't on the bare runner → run the job in the
   `bilelmoussaoui/flatpak-github-actions:gnome-47` container (+ venv for pip under PEP 668).
4. That container has no `gh` CLI → moved the release-attach to a separate `ubuntu-latest` job.

CI build now succeeds (~9.5 min). The bundle attaches to the release automatically on future tags.

## Commits pushed to `main`

| Commit | What |
|--------|------|
| `935d8ac` | fix: blank-window fix + version bump 0.9.2 + docs + RELEASE_NOTES |
| `d6ab741` | ci: tomlkit + node generator from git |
| `1482e21` | ci: run build inside GNOME 47 builder container |
| `fadbc47` | ci: attach bundle in a separate runner job |

---

## Local build cheat sheet (Linux)

```bash
# one-time tooling
sudo apt-get install -y flatpak flatpak-builder python3-pip pipx
pipx install "git+https://github.com/flatpak/flatpak-builder-tools.git#subdirectory=node"
pipx install flatpak-cargo-generator
flatpak remote-add --if-not-exists --user flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user -y flathub org.gnome.Platform//47 org.gnome.Sdk//47 \
  org.freedesktop.Sdk.Extension.rust-stable//24.08 org.freedesktop.Sdk.Extension.node22//24.08

# build + run
flatpak-node-generator npm package-lock.json -o flatpak/generated-node-sources.json
flatpak-cargo-generator  src-tauri/Cargo.lock  -o flatpak/generated-cargo-sources.json
flatpak-builder --user --install --force-clean build-dir flatpak/io.github.crisp_oddio.glogger.yml
flatpak run io.github.crisp_oddio.glogger
```

Non-fatal NVIDIA/sandbox noise (canberra, GBM/DRM) doesn't stop the app. If a GPU ever leaves the
window blank, launch with `--env=WEBKIT_DISABLE_DMABUF_RENDERER=1` (optionally `LIBGL_ALWAYS_SOFTWARE=1`).

---

## Open items / next steps

- **Windows installer not built for v0.9.2.** `release.yml` only runs on manual dispatch (not tag
  push), so v0.9.2 has the Flatpak but no `.exe`. Run the **Release** workflow from the Actions tab
  if you want the Windows installer for this version.
- The `v0.9.2` tag sits at `1482e21` (one commit behind `main`); the only thing not in the tag is
  the attach-job refactor (`fadbc47`), which already ran manually. No rebuild needed.
- Generated offline source manifests (`flatpak/generated-*.json`) and `.flatpak-builder/` are now
  gitignored — CI regenerates them each run.
