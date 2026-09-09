#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

FRAMES="${1:-100}"
PIN="${PIN:-12345678}"
OUTPUT_NAME="${OUTPUT_NAME:-HDMI-A-2}"
STATS_FILE="${2:-${ROOT_DIR}/.omo/mass-ulw-20260906/bench-results-omarchy.json}"
LOG_DIR="/tmp/erd-bench-omarchy"

mkdir -p "${LOG_DIR}" "$(dirname "${STATS_FILE}")"

export WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-wayland-1}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/1000}"
if [ -z "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]; then
    SIG=$(ls -1td /run/user/1000/hypr/*_* 2>/dev/null | head -n1 | xargs -r basename || true)
    if [ -n "$SIG" ]; then
        export HYPRLAND_INSTANCE_SIGNATURE="$SIG"
    fi
fi
export PKG_CONFIG_PATH="/home/indo/erd-ffmpeg7/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
export LD_LIBRARY_PATH="/home/indo/erd-ffmpeg7/lib:${LD_LIBRARY_PATH:-}"

echo "========================================================"
echo "  EclipticRD Latency Benchmark (Omarchy Linux)"
echo "  Target: ${FRAMES} frames on ${OUTPUT_NAME}"
echo "========================================================"

HOST_BIN="${ROOT_DIR}/clients/rust/target/debug/erd-host"
CLIENT_BIN="${ROOT_DIR}/clients/rust/target/debug/erd-client"

if [ ! -f "${HOST_BIN}" ] || [ ! -f "${CLIENT_BIN}" ]; then
    echo "Building erd-host and erd-client..."
    cargo build --manifest-path "${ROOT_DIR}/clients/rust/Cargo.toml" -p erd-host -p erd-app --bins
fi

pkill -9 erd-host || true
pkill -9 erd-client || true
sleep 0.5

echo "[1/3] Starting erd-host on output ${OUTPUT_NAME}..."
"${HOST_BIN}" \
    --bootstrap-pin "${PIN}" \
    --auto-approve \
    --output "${OUTPUT_NAME}" > "${LOG_DIR}/host.log" 2>&1 &
HOST_PID=$!

READY=0
for i in $(seq 1 50); do
    if ss -ltn 'sport = :19730' 2>/dev/null | grep -q '19730'; then
        READY=1
        break
    fi
    sleep 0.1
done

if [ "$READY" -ne 1 ]; then
    echo "ERROR: erd-host failed to start or listen on port 19730."
    tail -n 25 "${LOG_DIR}/host.log"
    kill -9 "${HOST_PID}" 2>/dev/null || true
    exit 1
fi

echo "[2/3] Running erd-client for ${FRAMES} frames..."
CLIENT_EXIT=0
"${CLIENT_BIN}" \
    --host 127.0.0.1 \
    --pin "${PIN}" \
    --frames "${FRAMES}" \
    --nudge-ms 50 \
    --stats-json "${STATS_FILE}" \
    --timeout-secs 45 > "${LOG_DIR}/client.log" 2>&1 || CLIENT_EXIT=$?

kill -9 "${HOST_PID}" 2>/dev/null || true
wait "${HOST_PID}" 2>/dev/null || true

if [ "$CLIENT_EXIT" -ne 0 ]; then
    echo "ERROR: erd-client exited with code ${CLIENT_EXIT}."
    echo "=== Host Log Tail ==="
    tail -n 20 "${LOG_DIR}/host.log"
    echo "=== Client Log Tail ==="
    tail -n 20 "${LOG_DIR}/client.log"
    exit "$CLIENT_EXIT"
fi

echo "[3/3] Parsing benchmark results..."
python3 - <<EOF
import json

stats_file = "${STATS_FILE}"
with open(stats_file, "r") as f:
    data = json.load(f)

frames = data.get("frames", 0)
p50_us = data.get("p50_us", 0)
p95_us = data.get("p95_us", 0)
p99_us = data.get("p99_us", 0)
max_us = data.get("max_us", 0)

rx = data.get("receiver_snapshot", {})
datagrams = rx.get("udp_authenticated_datagrams", 0)
bytes_rx = rx.get("udp_authenticated_bytes", 0)
bps = rx.get("udp_receive_bps") or 0.0
loss_ratio = rx.get("packet_loss_ratio") or 0.0
ready_stats = rx.get("host_ready_to_encode_us") or {}
encode_stats = rx.get("host_encode_us") or {}
send_stats = rx.get("host_encode_to_send_complete_us") or {}
assembly_stats = rx.get("receive_assembly_us") or {}

print("=" * 68)
print("  ECLIPTICRD LATENCY & TELEMETRY BENCHMARK REPORT")
print("=" * 68)
print(f"  Decoded Video Frames:    {frames}")
print(f"  Authenticated Datagrams: {datagrams} ({bytes_rx:,} bytes, {bps / 1_000_000:.2f} Mbps)")
print(f"  Packet Loss Ratio:       {loss_ratio:.4f}")
print("-" * 68)
print("  PER-STAGE TIMING BREAKDOWN (p50 / p95 / p99):")
print("-" * 68)
print(f"  1. Host Ready-to-Encode:  {ready_stats.get('p50_us', 0)/1000:7.2f} ms | {ready_stats.get('p95_us', 0)/1000:7.2f} ms | {ready_stats.get('p99_us', 0)/1000:7.2f} ms")
print(f"  2. Host Encode Duration:  {encode_stats.get('p50_us', 0)/1000:7.2f} ms | {encode_stats.get('p95_us', 0)/1000:7.2f} ms | {encode_stats.get('p99_us', 0)/1000:7.2f} ms")
print(f"  3. Host Encode->Send:     {send_stats.get('p50_us', 0)/1000:7.2f} ms | {send_stats.get('p95_us', 0)/1000:7.2f} ms | {send_stats.get('p99_us', 0)/1000:7.2f} ms")
print(f"  4. Receive Frame Assembly:{assembly_stats.get('p50_us', 0)/1000:7.2f} ms | {assembly_stats.get('p95_us', 0)/1000:7.2f} ms | {assembly_stats.get('p99_us', 0)/1000:7.2f} ms")
print(f"  5. Client Decode Time:    {p50_us/1000:7.2f} ms | {p95_us/1000:7.2f} ms | {p99_us/1000:7.2f} ms")
print("=" * 68)
EOF

echo "Benchmark statistics saved to ${STATS_FILE}."
