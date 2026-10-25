#!/usr/bin/env bash
# Makes the video player's test matrix: synthetic clips (test patterns and
# tones, nothing copyrighted) in every format the player must handle, plus
# subtitles in Persian, Arabic and English in old encodings, and a small
# "series" folder. Local testing only: the files are never committed or
# shipped, so this may use any FFmpeg on the PATH (Homebrew's has the GPL
# encoders x264/x265 that make realistic H.264/HEVC samples; the app's own
# FFmpeg does not and must not).
#
#   scripts/media-samples.sh [out dir]    # default: src-tauri/target/media-samples
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:-$repo/src-tauri/target/media-samples}"
ff="${FFMPEG:-ffmpeg}"
mkdir -p "$out/series"
cd "$out"
q=(-hide_banner -loglevel error -y)

# The subtitles' text. Our own sentences; one cue every 4 seconds.
python3 - "$out" <<'PY'
import sys, os
out = sys.argv[1]
lines = {
  "en": ["Good morning, how are you today?", "I left my keys on the kitchen table.",
         "The train to the city leaves at nine.", "Could you speak a little more slowly?",
         "We are going to the market after lunch.", "This book is more interesting than I thought.",
         "Please close the window, it is cold.", "My sister lives near the old bridge.",
         "Do you want tea or coffee?", "See you tomorrow at the station."],
  "fa": ["صبح بخیر، امروز حالت چطور است؟", "کلیدهایم را روی میز آشپزخانه گذاشتم.",
         "قطار شهر ساعت نه حرکت می‌کند.", "می‌توانی کمی آهسته‌تر صحبت کنی؟",
         "بعد از ناهار به بازار می‌رویم.", "این کتاب از آنچه فکر می‌کردم جالب‌تر است.",
         "لطفاً پنجره را ببند، هوا سرد است.", "خواهرم نزدیک پل قدیمی زندگی می‌کند.",
         "چای می‌خواهی یا قهوه؟", "فردا در ایستگاه می‌بینمت."],
  "ar": ["صباح الخير، كيف حالك اليوم؟", "تركت مفاتيحي على طاولة المطبخ.",
         "القطار إلى المدينة يغادر في التاسعة.", "هل يمكنك أن تتكلم ببطء قليلاً؟",
         "سنذهب إلى السوق بعد الغداء.", "هذا الكتاب أكثر إثارة مما ظننت.",
         "من فضلك أغلق النافذة، الجو بارد.", "تعيش أختي قرب الجسر القديم.",
         "هل تريد شاياً أم قهوة؟", "أراك غداً في المحطة."],
}
def ts(ms, sep=","):
    h, ms = divmod(ms, 3600000); m, ms = divmod(ms, 60000); s, ms = divmod(ms, 1000)
    return f"{h:02}:{m:02}:{s:02}{sep}{ms:03}"
def srt(lang, n=15):
    rows = []
    for i in range(n):
        a, b = i * 4000 + 500, i * 4000 + 3500
        rows.append(f"{i+1}\n{ts(a)} --> {ts(b)}\n{lines[lang][i % 10]}\n")
    return "\n".join(rows)
def old1256(text):
    # Windows-1256 has no Persian yeh (U+06CC): old Persian files carry the
    # Arabic yeh instead, exactly as made here.
    return text.replace("ی", "ي")
def write(name, text, enc, bom=False):
    data = text.replace("\n", "\r\n").encode(enc)
    if bom and enc == "utf-8": data = b"\xef\xbb\xbf" + data
    open(os.path.join(out, name), "wb").write(data)
# External subtitles beside movie.mkv: the encodings old files really use.
write("movie.fa.srt", old1256(srt("fa")), "cp1256")              # Windows-1256, the usual old Persian file
write("movie.ar.srt", srt("ar"), "utf-8", bom=True)      # UTF-8 with a BOM
write("movie.farsi.srt", srt("fa"), "utf-16")            # UTF-16 LE with a BOM
write("movie.srt", srt("en").replace("today", "today, café"), "cp1252")  # Windows-1252, no language in the name
write("movie.per.srt", srt("fa"), "utf-8")               # .per. hint
# The embedded tracks' sources.
write("embed-en.srt", srt("en"), "utf-8")
write("embed-fa-1256.srt", old1256(srt("fa")), "cp1256")
# WebVTT and ASS (with styling: bold, italic, colour, a positioned line).
vtt = ["WEBVTT", ""]
for i in range(15):
    a, b = i * 4000 + 500, i * 4000 + 3500
    vtt += [f"{ts(a, '.')} --> {ts(b, '.')} line:85%", f"<i>{lines['en'][i % 10]}</i>", ""]
open(os.path.join(out, "movie.en.vtt"), "w", encoding="utf-8").write("\n".join(vtt))
ass = """[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,64,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,3,1,2,40,40,50,1
Style: Sign,Arial,48,&H0000FFFF,&H000000FF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,2,0,8,40,40,40,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
"""
def assts(ms):
    h, ms = divmod(ms, 3600000); m, ms = divmod(ms, 60000); s, ms = divmod(ms, 1000)
    return f"{h}:{m:02}:{s:02}.{ms//10:02}"
for i in range(15):
    a, b = i * 4000 + 500, i * 4000 + 3500
    text = lines["en"][i % 10]
    if i % 3 == 1: text = "{\\b1}" + text.split(" ")[0] + "{\\b0} " + " ".join(text.split(" ")[1:])
    if i % 3 == 2: text = "{\\i1}" + text + "{\\i0}\\N{\\c&H00FF00&}second line"
    ass += f"Dialogue: 0,{assts(a)},{assts(b)},Default,,0,0,0,,{text}\n"
ass += f"Dialogue: 1,{assts(1000)},{assts(9000)},Sign,,0,0,0,,{{\\an8\\pos(960,80)}}A SIGN AT THE TOP\n"
ass += f"Dialogue: 0,{assts(12000)},{assts(14000)},Default,,0,0,0,,{{\\p1}}m 0 0 l 100 0 100 100 0 100{{\\p0}}\n"
open(os.path.join(out, "movie.en.ass"), "w", encoding="utf-8").write(ass)
open(os.path.join(out, "chapters.txt"), "w").write(""";FFMETADATA1
[CHAPTER]
TIMEBASE=1/1000
START=0
END=20000
title=Opening
[CHAPTER]
TIMEBASE=1/1000
START=20000
END=40000
title=Middle
[CHAPTER]
TIMEBASE=1/1000
START=40000
END=60000
title=End
""")
# The series' subtitles, named the way downloaded ones are.
for ep in (1, 2, 3):
    write(f"series/Show.Name.S01E0{ep}.fa.srt", old1256(srt("fa", 8)), "cp1256")
write("series/show name 1x02 English.srt", srt("en", 8), "utf-8")
PY

tone() { echo "sine=frequency=$1:sample_rate=48000:duration=$2"; }

# 1. MP4 H.264/AAC 1080p (direct play).
"$ff" "${q[@]}" -f lavfi -i "testsrc2=size=1920x1080:rate=30:duration=60" -f lavfi -i "$(tone 440 60)" \
  -c:v libx264 -preset veryfast -profile:v high -pix_fmt yuv420p -g 60 -c:a aac -b:a 128k \
  -movflags +faststart mp4_h264_aac_1080p.mp4

# 2. MKV H.264 + 2 audio (AC3 5.1 English, AAC stereo Persian) + 2 SRT (English UTF-8,
#    Persian Windows-1256, copied as raw bytes) + 3 chapters (remux + audio transcode).
"$ff" "${q[@]}" -f lavfi -i "testsrc2=size=1280x720:rate=24:duration=60" \
  -f lavfi -i "$(tone 330 60)" -f lavfi -i "$(tone 660 60)" \
  -sub_charenc UTF-8 -i embed-en.srt -i embed-fa-1256.srt -i chapters.txt \
  -map 0:v -map 1:a -map 2:a -map 3:s -map 4:s -map_metadata 5 -map_chapters 5 \
  -c:v libx264 -preset veryfast -pix_fmt yuv420p -g 48 \
  -c:a:0 ac3 -ac:a:0 6 -b:a:0 384k -c:a:1 aac -b:a:1 128k -c:s copy \
  -metadata:s:a:0 language=eng -metadata:s:a:0 title="English 5.1" \
  -metadata:s:a:1 language=per -metadata:s:a:1 title="Persian" \
  -metadata:s:s:0 language=eng -metadata:s:s:1 language=per \
  movie.mkv

# 3. MKV HEVC 10-bit 4K (video transcode).
"$ff" "${q[@]}" -f lavfi -i "testsrc2=size=3840x2160:rate=30:duration=20" -f lavfi -i "$(tone 500 20)" \
  -c:v libx265 -preset ultrafast -pix_fmt yuv420p10le -x265-params log-level=error -tag:v hvc1 \
  -c:a aac mkv_hevc10_4k.mkv

# 4. WebM VP9/Opus.
"$ff" "${q[@]}" -f lavfi -i "testsrc2=size=1280x720:rate=30:duration=30" -f lavfi -i "$(tone 550 30)" \
  -c:v libvpx-vp9 -deadline realtime -cpu-used 8 -b:v 2M -c:a libopus webm_vp9_opus.webm

# 5. AVI MPEG-4 Part 2 / MP3.
"$ff" "${q[@]}" -f lavfi -i "testsrc2=size=640x480:rate=25:duration=30" -f lavfi -i "$(tone 600 30)" \
  -c:v mpeg4 -vtag XVID -q:v 4 -c:a libmp3lame -b:a 128k avi_mpeg4_mp3.avi

# 6. MOV ProRes 422 + PCM (video transcode).
"$ff" "${q[@]}" -f lavfi -i "testsrc2=size=1920x1080:rate=25:duration=10" -f lavfi -i "$(tone 700 10)" \
  -c:v prores_ks -profile:v 2 -c:a pcm_s16le mov_prores.mov

# 7. HDR10 HEVC 4K: BT.2020 primaries, PQ transfer, mastering display and content light level.
"$ff" "${q[@]}" -f lavfi -i "testsrc2=size=3840x2160:rate=24:duration=15" -f lavfi -i "$(tone 450 15)" \
  -vf "format=yuv420p,zscale=tin=bt709:pin=bt709:min=bt709:rin=tv:transfer=smpte2084:primaries=bt2020:matrix=bt2020nc:range=tv:npl=400,format=yuv420p10le" \
  -c:v libx265 -preset ultrafast -tag:v hvc1 -x265-params \
  "log-level=error:hdr10=1:colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc:master-display=G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1):max-cll=1000,400" \
  -color_primaries bt2020 -color_trc smpte2084 -colorspace bt2020nc -c:a aac hdr10_hevc.mkv

# 8. MP4 with a mov_text subtitle and an MKV with an ASS track (extraction paths).
"$ff" "${q[@]}" -f lavfi -i "testsrc2=size=640x360:rate=25:duration=20" -f lavfi -i "$(tone 400 20)" -i embed-en.srt \
  -map 0 -map 1 -map 2 -c:v libx264 -preset veryfast -pix_fmt yuv420p -c:a aac -c:s mov_text \
  -metadata:s:s:0 language=eng mp4_movtext.mp4
"$ff" "${q[@]}" -f lavfi -i "testsrc2=size=640x360:rate=25:duration=20" -f lavfi -i "$(tone 400 20)" -i movie.en.ass \
  -map 0 -map 1 -map 2 -c:v libx264 -preset veryfast -pix_fmt yuv420p -c:a aac -c:s copy \
  -metadata:s:s:0 language=eng mkv_ass.mkv

# 9. A series: three short episodes in one folder (next / previous, resume, matched subtitles).
for ep in 1 2 3; do
  "$ff" "${q[@]}" -f lavfi -i "testsrc2=size=640x360:rate=25:duration=12" -f lavfi -i "$(tone $((300 + ep * 100)) 12)" \
    -c:v libx264 -preset veryfast -pix_fmt yuv420p -c:a aac "series/Show.Name.S01E0$ep.720p.mkv"
done
rm -f embed-en.srt embed-fa-1256.srt chapters.txt
ls -la "$out" "$out/series"
