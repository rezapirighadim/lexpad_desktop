#!/usr/bin/env bash
# Refuses an FFmpeg binary that is not LGPL-only: its configuration (as the
# binary itself prints it, or as found in its bytes when it cannot run here,
# such as a Windows .exe on the Mac) must not enable GPL, version3-only or
# nonfree code, nor any GPL library.
#
#   scripts/ffmpeg/check-lgpl.sh <ffmpeg or ffprobe binary>
set -euo pipefail
bin=$1
if conf=$("$bin" -hide_banner -buildconf 2>/dev/null); then
  :
else
  conf=$(strings -n 12 "$bin" | grep -m1 -- '--prefix=' || true)
fi
[[ -n "$conf" ]] || {
  echo "$bin: no build configuration found" >&2
  exit 1
}
bad=$(grep -oE -- '--enable-(gpl|nonfree|libx264|libx265|libxvid|libvidstab|frei0r|librubberband|libcdio|libdavs2|libxavs2?|avisynth|libfdk-aac)' <<<"$conf" | sort -u || true)
if [[ -n "$bad" ]]; then
  echo "$bin is not LGPL-only:" $bad >&2
  exit 1
fi
if version=$("$bin" -hide_banner -L 2>/dev/null); then
  grep -q "GNU Lesser General Public" <<<"$version" || {
    echo "$bin does not report the LGPL" >&2
    exit 1
  }
fi
echo "$bin: LGPL only"
