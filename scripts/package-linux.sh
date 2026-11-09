#!/usr/bin/env bash
# Build the Linux AppImage for a release.
#
#   ./scripts/package-linux.sh                          # official update server
#   RISQUETTE_UPDATE_URL=https://… ./scripts/package-linux.sh
#
# Writes dist/Risquette-<version>-linux-x86_64.AppImage and a matching .sha256.
# The update server finds these names on GitHub releases, and the app refuses
# updates without the checksum.
#
# Needs the build dependencies in docs/development.md. linuxdeploy (which bundles
# the libraries the app needs and builds the AppImage) is downloaded once,
# pinned to a release and checked against its SHA-256.
set -euo pipefail
cd "$(dirname "$0")/.."

LINUXDEPLOY_VERSION="1-alpha-20251107-1"
LINUXDEPLOY_SHA256="c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d"
ARCH="x86_64"

version="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "risquette"))')"
echo "Packaging Risquette ${version} for Linux"
if [[ -n "${RISQUETTE_UPDATE_URL:-}" ]]; then
  echo "Update server: ${RISQUETTE_UPDATE_URL}"
else
  echo "Update server: https://risquette.xnoid.studio (official)"
fi

cargo build --release --locked -p risquette

tools="target/appimage-tools"
linuxdeploy="${tools}/linuxdeploy-${LINUXDEPLOY_VERSION}-${ARCH}.AppImage"
mkdir -p "${tools}"
if [[ ! -x "${linuxdeploy}" ]]; then
  curl -fsSL -o "${linuxdeploy}.part" "https://github.com/linuxdeploy/linuxdeploy/releases/download/${LINUXDEPLOY_VERSION}/linuxdeploy-${ARCH}.AppImage"
  echo "${LINUXDEPLOY_SHA256}  ${linuxdeploy}.part" | sha256sum --check --quiet
  mv "${linuxdeploy}.part" "${linuxdeploy}"
  chmod +x "${linuxdeploy}"
fi

appdir="target/AppDir"
rm -rf "${appdir}"
icon="target/appimage-tools/risquette.png"
cp branding/logo-256.png "${icon}"

mkdir -p dist
name="Risquette-${version}-linux-${ARCH}.AppImage"
# Runs without FUSE (CI machines, containers) by extracting itself first.
export APPIMAGE_EXTRACT_AND_RUN=1
LDAI_OUTPUT="dist/${name}" "${linuxdeploy}" \
  --appdir "${appdir}" \
  --executable target/release/risquette \
  --desktop-file packaging/linux/risquette.desktop \
  --icon-file "${icon}" \
  --output appimage

(cd dist && sha256sum "${name}" > "${name}.sha256")
echo "Wrote dist/${name} ($(du -h "dist/${name}" | cut -f1))"
echo "Wrote dist/${name}.sha256"
