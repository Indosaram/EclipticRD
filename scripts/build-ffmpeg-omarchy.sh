#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 2 || $1 != /* || $2 != /* ]]; then
  printf 'Usage: bash %s NEW_ABSOLUTE_PREFIX NEW_ABSOLUTE_BUILD_DIRECTORY\n' "$0" >&2
  exit 2
fi
prefix=$(realpath -m -- "$1")
work=$(realpath -m -- "$2")
if [[ $(uname -s) != Linux || $(uname -m) != x86_64 ]]; then
  printf 'This pinned build targets Omarchy Linux x86_64.\n' >&2
  exit 2
fi
if [[ -e $prefix || -L $prefix || -e $work || -L $work || $prefix == "$work" || $prefix == "$work/"* || $work == "$prefix/"* ]]; then
  printf 'Prefix and build directory must be new, separate paths.\n' >&2
  exit 2
fi
for spec in x264:0.165.3222 libva:1.24.0 libdrm:2.4.134; do
  package=${spec%%:*}
  expected=${spec#*:}
  actual=$(pkg-config --modversion "$package")
  if [[ $actual != "$expected" ]]; then
    printf '%s: expected %s, found %s; review dependency pins before rebuilding.\n' "$package" "$expected" "$actual" >&2
    exit 1
  fi
done
mkdir -p -- "$(dirname "$prefix")" "$(dirname "$work")"
mkdir -- "$prefix" "$work"
cd "$work"

curl --fail --location --max-time 180 \
  https://ffmpeg.org/releases/ffmpeg-7.0.2.tar.xz -o ffmpeg.tar.xz
curl --fail --location --max-time 180 \
  https://codeload.github.com/FFmpeg/nv-codec-headers/tar.gz/refs/tags/n12.1.14.0 -o nv-codec-headers.tar.gz
curl --fail --location --max-time 180 \
  https://stable-mirror.omarchy.org/extra/os/x86_64/nasm-3.02-1-x86_64.pkg.tar.zst -o nasm.pkg.tar.zst
printf '%s\n' \
  '8646515b638a3ad303e23af6a3587734447cb8fc0a0c064ecdb8e95c4fd8b389  ffmpeg.tar.xz' \
  '2fefaa227d2a3b4170797796425a59d1dd2ed5fd231db9b4244468ba327acd0b  nv-codec-headers.tar.gz' \
  '3a749b14839e1ea665e42b991fa15dfb6e9fc871b8bf4f8a48a40b789edcf579  nasm.pkg.tar.zst' \
  | sha256sum --check
tar -xf ffmpeg.tar.xz
tar -xf nv-codec-headers.tar.gz
mkdir nasm
bsdtar -xf nasm.pkg.tar.zst -C nasm
export PATH="$work/nasm/usr/bin:$PATH"
make -C nv-codec-headers-n12.1.14.0 PREFIX="$work/codec-headers" install
export PKG_CONFIG_PATH="$work/codec-headers/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
cd ffmpeg-7.0.2
./configure \
  --prefix="$prefix" --disable-doc --disable-debug --disable-ffplay \
  --disable-network --disable-autodetect --enable-shared --disable-static \
  --enable-gpl --enable-libx264 --enable-vaapi --enable-ffnvcodec --enable-nvenc --enable-nonfree \
  --enable-x86asm
make -j"${ERD_BUILD_JOBS:-6}"
make install
export LD_LIBRARY_PATH="$prefix/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
"$prefix/bin/ffmpeg" -version
{
  printf 'FFmpeg source: 7.0.2\nNV codec headers: n12.1.14.0\n'
  sha256sum "$work/ffmpeg.tar.xz" "$work/nv-codec-headers.tar.gz" "$work/nasm.pkg.tar.zst"
  gcc --version
  nasm --version
  pkg-config --modversion x264 libva libdrm ffnvcodec
  "$prefix/bin/ffmpeg" -buildconf
  ldd "$prefix/bin/ffmpeg"
} > "$prefix/build-receipt.txt" 2>&1
printf 'FFMPEG_BUILD_PASS %s\n' "$prefix"
