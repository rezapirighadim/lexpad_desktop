#!/usr/bin/env bash
# Builds the FFmpeg that ships in Lexpad for macOS: ffmpeg and ffprobe,
# universal (arm64 + x86_64), LGPL 2.1 or later only, from the pinned sources
# in versions.env. The result goes to src-tauri/binaries/ as Tauri sidecars
# (ffmpeg-<target triple>, ffprobe-<target triple>, and -universal-apple-darwin).
#
#   scripts/ffmpeg/build-macos.sh            # build (about 10 minutes on an M-series Mac)
#   scripts/ffmpeg/build-macos.sh --check    # only check what is in src-tauri/binaries/
#
# Needs: Xcode command line tools, nasm, meson, ninja, pkg-config, autoconf,
# automake, libtool (brew install nasm meson ninja pkg-config autoconf automake libtool).
#
# What is in it (docs/ffmpeg.md): FFmpeg's own decoders, demuxers, muxers and
# filters; VideoToolbox (hardware decode and the h264_videotoolbox encoder) and
# AudioToolbox; zimg for zscale (HDR tone-mapping); dav1d for AV1. Not in it:
# anything GPL (x264, x265, ...) or nonfree, networking, capture devices
# (only the lavfi input, for test patterns), ffplay.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
out="$repo/src-tauri/binaries"
# shellcheck source=versions.env
source "$here/versions.env"

check() {
  local ok=1
  for tool in ffmpeg ffprobe; do
    for triple in aarch64-apple-darwin x86_64-apple-darwin universal-apple-darwin; do
      local bin="$out/$tool-$triple"
      if [[ ! -x "$bin" ]]; then
        echo "missing: $bin" >&2
        ok=0
        continue
      fi
    done
    local uni="$out/$tool-universal-apple-darwin"
    [[ -x "$uni" ]] || continue
    lipo -info "$uni" | grep -q "x86_64 arm64\|arm64 x86_64" || {
      echo "$uni is not universal" >&2
      ok=0
    }
    "$here/check-lgpl.sh" "$uni" || ok=0
  done
  [[ $ok == 1 ]]
}

if [[ "${1:-}" == "--check" ]]; then
  check
  exit
fi

for tool in nasm meson ninja pkg-config autoreconf glibtoolize lipo clang; do
  command -v "$tool" >/dev/null || {
    echo "needs $tool (brew install nasm meson ninja pkg-config autoconf automake libtool)" >&2
    exit 1
  }
done

work="${LEXPAD_FFMPEG_WORK:-$repo/src-tauri/target/ffmpeg-build}"
jobs="${LEXPAD_FFMPEG_JOBS:-6}" # kept low: the Mac also runs other work (RAM)
mkdir -p "$work/src" "$out"

fetch() { # url sha256 file
  local url=$1 sum=$2 file="$work/src/$3"
  if [[ ! -f "$file" ]]; then
    curl -fsSL --retry 3 -o "$file.part" "$url"
    mv "$file.part" "$file"
  fi
  echo "$sum  $file" | shasum -a 256 -c - >/dev/null || {
    echo "checksum mismatch: $file" >&2
    rm -f "$file"
    exit 1
  }
}

fetch "$FFMPEG_URL" "$FFMPEG_SHA256" "ffmpeg-$FFMPEG_VERSION.tar.xz"
fetch "$ZIMG_URL" "$ZIMG_SHA256" "zimg-$ZIMG_VERSION.tar.gz"
fetch "$DAV1D_URL" "$DAV1D_SHA256" "dav1d-$DAV1D_VERSION.tar.gz"

min_macos=11.0 # the app's own minimumSystemVersion (tauri.conf.json)

build_arch() { # arm64 | x86_64
  local arch=$1 host triple prefix build
  case $arch in
    arm64) host=aarch64-apple-darwin triple=aarch64-apple-darwin ;;
    x86_64) host=x86_64-apple-darwin triple=x86_64-apple-darwin ;;
  esac
  prefix="$work/prefix-$arch"
  build="$work/build-$arch"
  rm -rf "$prefix" "$build"
  mkdir -p "$prefix" "$build"
  local cflags="-arch $arch -mmacosx-version-min=$min_macos -O2"
  export PKG_CONFIG_PATH="$prefix/lib/pkgconfig"
  export PKG_CONFIG_LIBDIR="$prefix/lib/pkgconfig"

  # zimg (static)
  tar -xzf "$work/src/zimg-$ZIMG_VERSION.tar.gz" -C "$build"
  (
    cd "$build/zimg-release-$ZIMG_VERSION"
    ./autogen.sh >/dev/null
    ./configure --host="$host" --prefix="$prefix" --disable-shared --enable-static \
      CC="clang" CXX="clang++" CFLAGS="$cflags" CXXFLAGS="$cflags" LDFLAGS="-arch $arch" >/dev/null
    make -j"$jobs" >/dev/null
    make install >/dev/null
  )

  # dav1d (static)
  tar -xzf "$work/src/dav1d-$DAV1D_VERSION.tar.gz" -C "$build"
  local cross="$build/meson-$arch.ini"
  cat >"$cross" <<EOF
[binaries]
c = ['clang', '-arch', '$arch', '-mmacosx-version-min=$min_macos']
ar = 'ar'
strip = 'strip'
nasm = 'nasm'
[host_machine]
system = 'darwin'
cpu_family = '$([[ $arch == arm64 ]] && echo aarch64 || echo x86_64)'
cpu = '$arch'
endian = 'little'
EOF
  (
    cd "$build/dav1d-$DAV1D_VERSION"
    meson setup out --cross-file "$cross" --prefix="$prefix" --libdir=lib \
      --buildtype=release -Ddefault_library=static -Denable_tools=false -Denable_tests=false >/dev/null
    ninja -C out -j"$jobs" >/dev/null
    ninja -C out install >/dev/null
  )

  # FFmpeg: LGPL only. --disable-autodetect so nothing is picked up from the
  # machine it is built on (Homebrew's libraries); everything is named here.
  tar -xJf "$work/src/ffmpeg-$FFMPEG_VERSION.tar.xz" -C "$build"
  local cross_args=()
  [[ $arch == "$(uname -m)" ]] || cross_args=(--enable-cross-compile)
  (
    cd "$build/ffmpeg-$FFMPEG_VERSION"
    ./configure \
      --prefix="$prefix" --arch="$arch" --target-os=darwin ${cross_args[@]+"${cross_args[@]}"} \
      --cc="clang -arch $arch" --cxx="clang++ -arch $arch" \
      --extra-cflags="-mmacosx-version-min=$min_macos -I$prefix/include" \
      --extra-ldflags="-mmacosx-version-min=$min_macos -L$prefix/lib" \
      --extra-libs="-lc++ -liconv" \
      --pkg-config=pkg-config --pkg-config-flags=--static \
      --disable-gpl --disable-nonfree --disable-version3 \
      --disable-autodetect --disable-debug --disable-doc --disable-ffplay \
      --disable-network --disable-devices --enable-indev=lavfi --disable-shared --enable-static \
      --enable-pthreads --enable-zlib --enable-bzlib --enable-iconv \
      --enable-videotoolbox --enable-audiotoolbox \
      --enable-libzimg --enable-libdav1d \
      >"$build/ffmpeg-configure.log"
    make -j"$jobs" ffmpeg ffprobe >"$build/ffmpeg-make.log" 2>&1
    strip -x ffmpeg ffprobe
    cp ffmpeg "$out/ffmpeg-$triple"
    cp ffprobe "$out/ffprobe-$triple"
    cp ffbuild/config.mak "$out/ffmpeg-$triple.config.mak"
  )
}

build_arch arm64
build_arch x86_64
for tool in ffmpeg ffprobe; do
  lipo -create "$out/$tool-aarch64-apple-darwin" "$out/$tool-x86_64-apple-darwin" \
    -output "$out/$tool-universal-apple-darwin"
done
check
echo "built FFmpeg $FFMPEG_VERSION (LGPL) into $out"
