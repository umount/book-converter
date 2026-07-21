#!/usr/bin/env bash
# Download the prebuilt pdfium dynamic library for the current platform into
# src-tauri/pdfium/, where the app loads it (bundled as a Tauri resource) for
# cross-platform PDF text extraction. Source: bblanchon/pdfium-binaries.
set -euo pipefail

DEST="$(cd "$(dirname "$0")/.." && pwd)/src-tauri/pdfium"
BASE="https://github.com/bblanchon/pdfium-binaries/releases/latest/download"

os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
  Linux)  plat="linux";  libname="libpdfium.so";    libpath="lib/libpdfium.so" ;;
  Darwin) plat="mac";    libname="libpdfium.dylib"; libpath="lib/libpdfium.dylib" ;;
  MINGW*|MSYS*|CYGWIN*) plat="win"; libname="pdfium.dll"; libpath="bin/pdfium.dll" ;;
  *) echo "Unsupported OS: $os" >&2; exit 1 ;;
esac

case "$arch" in
  x86_64|amd64) cpu="x64" ;;
  arm64|aarch64) cpu="arm64" ;;
  *) echo "Unsupported arch: $arch" >&2; exit 1 ;;
esac

asset="pdfium-${plat}-${cpu}.tgz"

if [ -f "$DEST/$libname" ]; then
  echo "pdfium already present: $DEST/$libname"
  exit 0
fi

echo "Fetching $asset ..."
mkdir -p "$DEST"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
curl -fsSL -o "$tmp/pdfium.tgz" "$BASE/$asset"
tar xzf "$tmp/pdfium.tgz" -C "$tmp"
cp -f "$tmp/$libpath" "$DEST/$libname"
echo "Installed: $DEST/$libname"
