#!/usr/bin/env bash
# Puts the pinned LGPL static FFmpeg from BtbN/FFmpeg-Builds into
# src-tauri/binaries/ as Tauri sidecars, checked against versions.env.
#
#   scripts/ffmpeg/fetch-prebuilt.sh win64     # Windows x64 (CI's release job, or a cross check)
#   scripts/ffmpeg/fetch-prebuilt.sh linux64   # Linux x64 (CI's check job: tests only, never shipped)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
out="$repo/src-tauri/binaries"
# shellcheck source=versions.env
source "$here/versions.env"

case "${1:-}" in
  win64) name=$BTBN_WIN64 sum=$BTBN_WIN64_SHA256 ext=zip triple=x86_64-pc-windows-msvc exe=.exe ;;
  linux64) name=$BTBN_LINUX64 sum=$BTBN_LINUX64_SHA256 ext=tar.xz triple=x86_64-unknown-linux-gnu exe= ;;
  *)
    echo "usage: $0 win64|linux64" >&2
    exit 2
    ;;
esac

if [[ -f "$out/ffmpeg-$triple$exe" && -f "$out/ffprobe-$triple$exe" && -f "$out/.$name" ]]; then
  echo "already there: $name"
  exit 0
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
file="$work/$name.$ext"
curl -fsSL --retry 3 -o "$file" "https://github.com/BtbN/FFmpeg-Builds/releases/download/$BTBN_TAG/$name.$ext"
if command -v sha256sum >/dev/null; then
  echo "$sum  $file" | sha256sum -c - >/dev/null
else
  echo "$sum  $file" | shasum -a 256 -c - >/dev/null
fi
mkdir -p "$work/x" "$out"
if [[ $ext == zip ]]; then
  unzip -q "$file" -d "$work/x"
else
  tar -xJf "$file" -C "$work/x"
fi
cp "$work/x/$name/bin/ffmpeg$exe" "$out/ffmpeg-$triple$exe"
cp "$work/x/$name/bin/ffprobe$exe" "$out/ffprobe-$triple$exe"
chmod +x "$out/ffmpeg-$triple$exe" "$out/ffprobe-$triple$exe"
"$here/check-lgpl.sh" "$out/ffmpeg-$triple$exe"
touch "$out/.$name"
echo "fetched $name"
