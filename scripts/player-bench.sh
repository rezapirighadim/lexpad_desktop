#!/usr/bin/env bash
# The media engine's benchmark on this Mac: builds the development page
# (cargo feature player-dev), plays every video in a folder through it, and
# reports start-up to first frame, seek latency (new run and buffered), and
# CPU and memory while playing (the app, its FFmpeg, and the WebKit
# processes it started). Never touches an installed Lexpad, which may run
# beside it.
#
#   scripts/player-bench.sh [folder]   # default: src-tauri/target/media-samples (scripts/media-samples.sh)
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
dir="${1:-$repo/src-tauri/target/media-samples}"
out="$repo/src-tauri/target/player-bench"
mkdir -p "$out"
rm -f "$out/reports.jsonl" "$out/samples.tsv"
if [[ "${SKIP_BUILD:-}" != 1 ]]; then
  (cd "$repo" && LEXPAD_PLAYER_DEV=1 pnpm tauri build --features player-dev --no-bundle >/dev/null)
fi
bin="$repo/src-tauri/target/release/lexpad-desktop"
before=$(pgrep -f 'com.apple.WebKit' | sort || true)
RUST_BACKTRACE=1 LEXPAD_PLAYER_BENCH="$dir" LEXPAD_PLAYER_BENCH_OUT="$out/reports.jsonl" "$bin" >"$out/stdout.log" 2>&1 &
app=$!
trap 'kill $app 2>/dev/null || true' EXIT
deadline=$((SECONDS + 900))
while ! grep -q '"done":true' "$out/reports.jsonl" 2>/dev/null; do
  ((SECONDS < deadline)) || { echo "timed out" >&2; exit 1; }
  kill -0 $app 2>/dev/null || { echo "the app quit" >&2; tail -20 "$out/stdout.log"; exit 1; }
  now=$(python3 -c 'import time;print(int(time.time()*1000))')
  webkit=$(comm -13 <(echo "$before") <(pgrep -f 'com.apple.WebKit' | sort) | tr '\n' ',' | sed 's/,$//')
  ffm=$(pgrep -P $app | tr '\n' ',' | sed 's/,$//' || true)
  sample() { [[ -n "$1" ]] && ps -o rss=,%cpu= -p "$1" | awk '{r+=$1; c+=$2} END {printf "%d\t%.1f", r/1024, c}' || printf "0\t0"; }
  printf "%s\t%s\t%s\t%s\n" "$now" "$(sample $app)" "$(sample "$ffm")" "$(sample "$webkit")" >>"$out/samples.tsv"
  sleep 1
done
python3 - "$out" <<'PY'
import json, sys
out = sys.argv[1]
reports = [json.loads(l) for l in open(f"{out}/reports.jsonl")]
samples = [l.rstrip("\n").split("\t") for l in open(f"{out}/samples.tsv")]
rows = [(int(s[0]), *map(float, s[1:])) for s in samples if len(s) == 7]
soaks = {r["report"]["file"]: (r["t"], r["report"]["ms"]) for r in reports if r["report"].get("phase") == "soak"}
print(f"{'file':28} {'mode':15} {'first frame':>11} {'seeks (new run) ms':>24} {'buf':>5} {'audio':>6} {'app MB/cpu':>11} {'ffmpeg MB/cpu':>14} {'webkit MB/cpu':>14} {'drop':>9}")
for r in reports:
    x = r["report"]
    if "seeks" not in x:
        continue
    t0, ms = soaks.get(x["file"], (0, 0))
    # The steady state: the second half of the soak (the first fills the buffer).
    win = [s for s in rows if t0 + ms / 2 <= s[0] <= t0 + ms]
    avg = lambda i: sum(s[i] for s in win) / len(win) if win else 0
    seeks = ",".join(str(s["ms"]) for s in x["seeks"])
    drop = f"{x['droppedFrames']}/{x['totalFrames']}" if x.get("totalFrames") else "-"
    print(f"{x['file'][:28]:28} {x['mode']:15} {x['firstFrameMs']:>9}ms {seeks:>24} {x['bufferedSeekMs']:>5} {str(x.get('audioSwitchMs') or '-'):>6} {avg(1):>5.0f}/{avg(2):>4.0f}% {avg(3):>7.0f}/{avg(4):>4.0f}% {avg(5):>7.0f}/{avg(6):>4.0f}% {drop:>9}" + (f"  ERROR {x['error']}" if x.get("error") else ""))
PY
