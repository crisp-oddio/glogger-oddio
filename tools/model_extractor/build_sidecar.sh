#!/usr/bin/env bash
# Freeze the model extractor (extract.py) into a standalone binary and place it
# where Tauri's `externalBin` expects the sidecar:
#   src-tauri/binaries/model-extractor-<target-triple>[.exe]
#
# Usage: tools/model_extractor/build_sidecar.sh <rust-target-triple>
#   e.g. x86_64-pc-windows-msvc | aarch64-apple-darwin | x86_64-unknown-linux-gnu
#
# UnityPy pulls native texture decoders + an fmod DLL + archspec's CPU JSON;
# all must be collected or the frozen exe fails at runtime.
set -euo pipefail

TRIPLE="${1:?usage: build_sidecar.sh <target-triple>}"
cd "$(dirname "$0")/../.."   # repo root

EXT=""
case "$TRIPLE" in *windows*) EXT=".exe" ;; esac

python -m pip install --quiet --disable-pip-version-check \
  pyinstaller -r tools/model_extractor/requirements.txt

python -m PyInstaller --onefile --noconfirm --clean --name model-extractor \
  --collect-all UnityPy \
  --collect-all fmod_toolkit \
  --collect-all texture2ddecoder \
  --collect-all etcpak \
  --collect-all astc_encoder_py \
  --collect-all archspec \
  --collect-all pygltflib \
  --distpath .pyi-out --workpath .pyi-build --specpath .pyi-build \
  tools/model_extractor/extract.py

mkdir -p src-tauri/binaries
mv ".pyi-out/model-extractor${EXT}" "src-tauri/binaries/model-extractor-${TRIPLE}${EXT}"
echo "sidecar → src-tauri/binaries/model-extractor-${TRIPLE}${EXT}"
