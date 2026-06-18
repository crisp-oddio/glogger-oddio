## What's Changed since v0.9.1

### Fixes
- fix: Flatpak production build now embeds the frontend assets instead of pointing
  the webview at the dev server URL, which left a blank white window. The bundle
  also pre-fetches the offline npm cache from the correct path and links the Node
  SDK headers before `npm ci`, so the sandboxed build succeeds end to end.

### Docs
- docs: added [docs/flatpak-build.md](docs/flatpak-build.md) covering local Flatpak
  builds, offline source generation, and the rendering fixes.

---
*Flatpak packaging is verified working on Linux (Debian 13, X11, NVIDIA).*
