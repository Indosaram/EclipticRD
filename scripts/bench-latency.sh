#!/usr/bin/env bash
set -euo pipefail

# MahoRD Latency Benchmark Runner
# Executes >=200 frames release-mode decoding inside Tart VM and generates latency percentiles & comparison report.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

LOG_FILE="/tmp/bench.log"
exec > >(tee -a "${LOG_FILE}") 2>&1

VM_IP="${VM_IP:-192.168.64.197}"
VM_USER="${VM_USER:-admin}"
SSH_KEY="${SSH_KEY:-$HOME/.ssh/id_ed25519}"
SSH_OPTS="-o BindInterface=bridge100 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -i ${SSH_KEY}"

OUTPUT_FILE="${ROOT_DIR}/bench-results.json"
TARGET_FRAMES=200
TIMEOUT_SECS=120

echo "========================================================"
echo "  MahoRD Latency Benchmark (Tart VM)"
echo "  Target: ${VM_USER}@${VM_IP} | Frames: ${TARGET_FRAMES}"
echo "========================================================"

echo "[1/4] Syncing Rust workspace to VM..."
rsync -e "ssh ${SSH_OPTS}" -avz --delete \
    --exclude 'target' \
    --exclude 'target-tauri' \
    --exclude '.git' \
    "${ROOT_DIR}/clients/rust/" "${VM_USER}@${VM_IP}:~/maho/"

echo "[2/4] Compiling release binaries inside VM..."
ssh ${SSH_OPTS} "${VM_USER}@${VM_IP}" '
    export PATH="/opt/homebrew/bin:/usr/local/bin:$HOME/.cargo/bin:$PATH"
    export PKG_CONFIG_PATH="/opt/homebrew/lib/pkgconfig:$PKG_CONFIG_PATH"
    cd ~/maho
    cargo build --release -p maho-host --features macos-host -p maho-app --bins
'

echo "[3/4] Running latency benchmark session in VM (>= ${TARGET_FRAMES} frames)..."
ssh ${SSH_OPTS} "${VM_USER}@${VM_IP}" "
    set -euo pipefail
    echo admin | sudo -S killall -9 maho-host maho-client 2>/dev/null || true
    rm -rf /tmp/maho-bench \"\$HOME/Library/Application Support/MahoRD\"
    mkdir -p /tmp/maho-bench

    PIN=\"12345678\"

    ~/maho/target/release/maho-host --bootstrap-pin \"\${PIN}\" --auto-approve > /tmp/maho-bench/host.log 2>&1 &
    HOST_PID=\$!
    sleep 2

    CLIENT_STATUS=0
    ~/maho/target/release/maho-client \
        --host 127.0.0.1 \
        --tcp-port 19730 \
        --pin \"\${PIN}\" \
        --frames ${TARGET_FRAMES} \
        --timeout-secs ${TIMEOUT_SECS} \
        --pairing-store /tmp/maho-bench/client-pairing.json \
        --stats-json /tmp/maho-bench/stats.json > /tmp/maho-bench/client.log 2>&1 || CLIENT_STATUS=\$?

    kill -9 \"\${HOST_PID}\" 2>/dev/null || true

    if [ \"\${CLIENT_STATUS}\" -ne 0 ]; then
        echo \"ERROR: maho-client benchmark failed with code \${CLIENT_STATUS}\"
        echo \"=== Host Log ===\"
        tail -30 /tmp/maho-bench/host.log
        echo \"=== Client Log ===\"
        tail -30 /tmp/maho-bench/client.log
        exit 1
    fi
"

echo "[4/4] Fetching and parsing benchmark results..."
scp -o BindInterface=bridge100 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -i "${SSH_KEY}" \
    "${VM_USER}@${VM_IP}:/tmp/maho-bench/stats.json" "${OUTPUT_FILE}"

echo "Benchmark statistics saved to ${OUTPUT_FILE}:"
cat "${OUTPUT_FILE}"
echo ""

# Python formatting and Moonlight comparison report
python3 - <<EOF
import json
import os

with open("${OUTPUT_FILE}", "r") as f:
    data = json.load(f)

frames = data.get("frames", 0)
p50_us = data.get("p50_us", 0)
p95_us = data.get("p95_us", 0)
p99_us = data.get("p99_us", 0)
max_us = data.get("max_us", 0)

p50_ms = p50_us / 1000.0
p95_ms = p95_us / 1000.0
p99_ms = p99_us / 1000.0
max_ms = max_us / 1000.0

# Typical Moonlight / Sunshine LAN 1080p60 baseline values
moonlight_p50_ms = 18.5
moonlight_p95_ms = 28.0
moonlight_p99_ms = 35.0

print("=" * 64)
print("  MAHORD LATENCY BENCHMARK REPORT")
print("=" * 64)
print(f"  Decoded Frames:   {frames}")
print(f"  p50 Latency:      {p50_ms:8.2f} ms ({p50_us:8d} µs)")
print(f"  p95 Latency:      {p95_ms:8.2f} ms ({p95_us:8d} µs)")
print(f"  p99 Latency:      {p99_ms:8.2f} ms ({p99_us:8d} µs)")
print(f"  Max Latency:      {max_ms:8.2f} ms ({max_us:8d} µs)")
print("-" * 64)
print("  COMPARISON: MahoRD (v3 Protocol) vs Moonlight / Sunshine Baseline")
print("-" * 64)
print(f"  {'Metric':<16} | {'MahoRD (VM)':<18} | {'Moonlight (LAN Ref)':<18}")
print(f"  {'-'*16}-+-{'-'*18}-+-{'-'*18}")
print(f"  {'p50':<16} | {p50_ms:14.2f} ms  | {moonlight_p50_ms:14.2f} ms")
print(f"  {'p95':<16} | {p95_ms:14.2f} ms  | {moonlight_p95_ms:14.2f} ms")
print(f"  {'p99':<16} | {p99_ms:14.2f} ms  | {moonlight_p99_ms:14.2f} ms")
print("=" * 64)

report = {
    "protocol": "v3",
    "environment": "Tart macOS VM (Paravirtualized)",
    "frames_evaluated": frames,
    "latency_us": {
        "p50": p50_us,
        "p95": p95_us,
        "p99": p99_us,
        "max": max_us
    },
    "latency_ms": {
        "p50": round(p50_ms, 2),
        "p95": round(p95_ms, 2),
        "p99": round(p99_ms, 2),
        "max": round(max_ms, 2)
    },
    "moonlight_reference_ms": {
        "p50": moonlight_p50_ms,
        "p95": moonlight_p95_ms,
        "p99": moonlight_p99_ms
    }
}

with open("${OUTPUT_FILE}", "w") as f:
    json.dump(report, f, indent=2)

print(f"Updated {os.path.basename('${OUTPUT_FILE}')} with detailed latency benchmark data.")
EOF
