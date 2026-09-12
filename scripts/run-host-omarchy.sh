#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

PIN="${PIN:-12345678}"
OUTPUT_NAME="${OUTPUT_NAME:-HDMI-A-2}"

export WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-wayland-1}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/1000}"
if [ -z "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]; then
    SIG=$(ls -1td /run/user/1000/hypr/*_* 2>/dev/null | head -n1 | xargs -r basename || true)
    if [ -n "$SIG" ]; then
        export HYPRLAND_INSTANCE_SIGNATURE="$SIG"
    fi
fi
export PKG_CONFIG_PATH="/home/indo/maho-ffmpeg7/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
export LD_LIBRARY_PATH="/home/indo/maho-ffmpeg7/lib:${LD_LIBRARY_PATH:-}"

HOST_BIN="${ROOT_DIR}/clients/rust/target/debug/maho-host"

if [ ! -f "${HOST_BIN}" ]; then
    echo "Building maho-host..."
    cargo build --manifest-path "${ROOT_DIR}/clients/rust/Cargo.toml" -p maho-host
fi

echo "Starting maho-host on output ${OUTPUT_NAME} (PIN: ${PIN})..."
exec "${HOST_BIN}" \
    --bootstrap-pin "${PIN}" \
    --auto-approve \
    --output "${OUTPUT_NAME}" "$@"
