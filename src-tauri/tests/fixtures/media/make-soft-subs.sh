#!/usr/bin/env bash
# Remakes soft-subs.mkv (the embedded-subtitle integration test's film, under
# 100 KB): five seconds of FFmpeg's test pattern and a tone, with three text
# subtitle tracks made from the files beside this script:
#   #2 fa.srt      SubRip, UTF-8, language "per", title "ValaMovie.Com", default + forced
#   #3 en.ass      ASS, language "eng", title "English SDH", hearing impaired
#   #4 fa1256.srt  SubRip in Windows-1256 (Arabic yeh and kaf), language "fas"
# Usage: make-soft-subs.sh [ffmpeg]   (default: src-tauri/binaries' copy for this Mac)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
ffmpeg="${1:-$here/../../../binaries/ffmpeg-$(uname -m | sed 's/arm64/aarch64/')-apple-darwin}"
cd "$here"
"$ffmpeg" -hide_banner -loglevel error -y \
  -f lavfi -i "testsrc2=size=160x90:rate=10:duration=5" \
  -f lavfi -i "sine=frequency=440:sample_rate=22050:duration=5" \
  -i fa.srt -i en.ass -i fa1256.srt \
  -map 0 -map 1 -map 2 -map 3 -map 4 \
  -c:v mpeg4 -q:v 10 -c:a aac -b:a 32k -c:s copy \
  -metadata:s:s:0 language=per -metadata:s:s:0 title=ValaMovie.Com -disposition:s:0 default+forced \
  -metadata:s:s:1 language=eng -metadata:s:s:1 "title=English SDH" -disposition:s:1 hearing_impaired \
  -metadata:s:s:2 language=fas -disposition:s:2 0 \
  soft-subs.mkv
