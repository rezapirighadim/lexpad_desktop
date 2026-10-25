# FFmpeg in Lexpad (the video player's media engine)

Lexpad for Mac and Windows ships `ffmpeg` and `ffprobe` beside its own executable (Tauri
sidecars, `bundle.externalBin` in `src-tauri/tauri.conf.json`). The video player uses them to
read any video (`src-tauri/src/media/`): ffprobe says what is in a file; ffmpeg copies or
converts it into the one stream both webviews play (H.264 + AAC in fragmented MP4), extracts
subtitle tracks and tone-maps HDR. They run as separate programs; Lexpad does not link FFmpeg.

## Licence: LGPL only (no GPL, no nonfree)

- **macOS**: LGPL 2.1 or later. Configured without `--enable-gpl`, `--enable-version3` or
  `--enable-nonfree`.
- **Windows**: LGPL 3.0 or later. BtbN's "lgpl" builds are configured with
  `--enable-version3` (checked in the binary's own configuration), which makes the whole
  LGPL 3.0 or later. Still no GPL and no nonfree part. LGPL 3's
  "installation information" clause is about consumer devices with locked firmware; on a PC
  the user replaces the file freely.

Neither has any GPL library (no x264, x265, Xvid, rubberband, vidstab,
frei0r, ...). `ffmpeg -L` prints the GNU Lesser General Public License, and
`scripts/ffmpeg/check-lgpl.sh` refuses any binary whose configuration says otherwise (the
build and fetch scripts run it).

H.264 is made by the platform's hardware encoder (VideoToolbox on macOS; NVENC, Quick Sync,
AMF or Media Foundation on Windows), or by OpenH264 (BSD-2-Clause) where none works. x264 is
never used: it is GPL.

What the LGPL asks of us, and how it is met:

- **The licence text** ships inside the binaries (`ffmpeg -L` prints it). TODO for the
  player's UI: an about line that names FFmpeg, its licence and this page (`media_engine`
  returns the version and the licence).
- **The source code** of exactly what ships: FFmpeg 9.0.2 from
  <https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz> (SHA-256 in `scripts/ffmpeg/versions.env`),
  zimg 3.0.6 and dav1d 1.5.4 from the addresses there, and the scripts in `scripts/ffmpeg/`
  that build them with these exact options. The Windows build's source and build scripts are
  BtbN's: <https://github.com/BtbN/FFmpeg-Builds> at the release named in `versions.env`.
  Lexpad's repository is public, so all of this is a link away; on request we also send the
  source tarballs.
- **Replacing it**: the user may swap either program for their own build (they are plain
  files beside the app's executable). Note that on macOS this breaks the app's code signature.

## The builds

| Platform              | Where from                                                                                                         | Script                                     |
| --------------------- | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------ |
| macOS (arm64, x86_64) | built from source on the Mac, universal (`lipo`)                                                                   | `scripts/ffmpeg/build-macos.sh`            |
| Windows x64           | BtbN's LGPL static build, monthly release, SHA-256 pinned (about 134 MB per program: every library BtbN builds in) | `scripts/ffmpeg/fetch-prebuilt.sh win64`   |
| Linux x64 (CI only)   | the same, for CI's build and tests; Linux is not shipped                                                           | `scripts/ffmpeg/fetch-prebuilt.sh linux64` |

The macOS build: FFmpeg's own decoders, demuxers, muxers and filters; VideoToolbox (hardware
decoding and `h264_videotoolbox`) and AudioToolbox; zimg (WTFPL) for `zscale`; dav1d
(BSD-2-Clause) for AV1; zlib, bzip2 and iconv from the SDK. Off: networking, capture devices
(only the `lavfi` input, for test patterns), ffplay, documentation. About 23 MB per
architecture per program; the universal pair is about 100 MB.

The binaries are never committed (`src-tauri/binaries/` is ignored). Before building the app:

```sh
scripts/ffmpeg/build-macos.sh          # on the Mac (about 3 minutes on an M-series Mac)
scripts/ffmpeg/fetch-prebuilt.sh win64 # CI's Windows job does this
```

To move to a newer FFmpeg: change `versions.env` (version, address, SHA-256; for Windows a
monthly BtbN release's tag and file names and their SHA-256 from its `checksums.sha256`), run
both scripts, run the tests and `scripts/player-bench.sh`.

## Patents

Decoding and encoding H.264, HEVC, AAC and the rest may need patent licences in some
countries. The hardware encoders and decoders are the operating system's and the GPU
vendor's, licensed with them. OpenH264's patent licence covers only Cisco's own binary; a
build of OpenH264 inside FFmpeg (as BtbN's) is not covered by it. It is used only on a
Windows machine where no hardware H.264 encoder works.
