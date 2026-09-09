#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 || $1 != /* ]]; then
  printf 'Usage: bash %s ABSOLUTE_FFMPEG_PREFIX COMMAND [ARGUMENTS...]\n' "$0" >&2
  exit 2
fi
prefix=$(realpath -e -- "$1")
shift
test -x "$prefix/bin/ffmpeg"
export PKG_CONFIG_PATH="$prefix/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export LD_LIBRARY_PATH="$prefix/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
for spec in libavcodec:61.3.100 libavutil:59.8.100 libswscale:8.1.100; do
  package=${spec%%:*}
  expected=${spec#*:}
  if [[ $(pkg-config --modversion "$package") != "$expected" || $(pkg-config --variable=pcfiledir "$package") != "$prefix/lib/pkgconfig" ]]; then
    printf 'Unexpected %s headers/version; refusing mixed FFmpeg builds.\n' "$package" >&2
    exit 1
  fi
done
configuration=$("$prefix/bin/ffmpeg" -hide_banner -buildconf 2>&1)
if [[ $configuration != *--enable-x86asm* || $configuration == *--disable-x86asm* ]]; then
  printf 'FFmpeg x86 assembly is not enabled.\n' >&2
  exit 1
fi
links=$(ldd "$prefix/bin/ffmpeg")
if ! awk -v prefix="$prefix/lib/" '
  /lib(avcodec|avformat|avutil|avfilter|avdevice|swscale|swresample|postproc)[.]so/ {
    count++;
    if (index($3, prefix) != 1) bad=1;
  }
  END { exit (bad || count < 3) }
' <<< "$links"; then
  printf 'FFmpeg runtime libraries do not all resolve inside %s/lib.\n%s\n' "$prefix" "$links" >&2
  exit 1
fi
exec "$@"
