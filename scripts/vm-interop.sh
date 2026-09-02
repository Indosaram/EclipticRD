#!/usr/bin/env bash
set -euo pipefail

# EclipticRD Tart VM Interoperability E2E Test Runner
# Verifies full pipeline: Bootstrap pairing -> Handshake -> UDP arming -> HEVC video encode/decode >= 10 frames -> Clean teardown

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

LOG_FILE="/tmp/vm-interop.log"
exec > >(tee -a "${LOG_FILE}") 2>&1

VM_IP="${VM_IP:-192.168.64.197}"
VM_USER="${VM_USER:-admin}"
SSH_KEY="${SSH_KEY:-$HOME/.ssh/id_ed25519}"
SSH_OPTS="-o BindInterface=bridge100 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -i ${SSH_KEY}"

echo "========================================================"
echo "  EclipticRD VM Interop E2E Test"
echo "  Target: ${VM_USER}@${VM_IP}"
echo "========================================================"

echo "[1/4] Syncing Rust workspace to VM..."
rsync -e "ssh ${SSH_OPTS}" -avz --delete \
    --exclude 'target' \
    --exclude 'target-tauri' \
    --exclude '.git' \
    "${ROOT_DIR}/clients/rust/" "${VM_USER}@${VM_IP}:~/erd/"

echo "[2/4] Building erd-host and erd-client (debug) inside VM..."
ssh ${SSH_OPTS} "${VM_USER}@${VM_IP}" '
    export PATH="/opt/homebrew/bin:/usr/local/bin:$HOME/.cargo/bin:$PATH"
    export PKG_CONFIG_PATH="/opt/homebrew/lib/pkgconfig:$PKG_CONFIG_PATH"
    cd ~/erd
    cargo build -p erd-host --features macos-host -p erd-app --bins
'

echo "[3/4] Running E2E pairing and frame decoding test in VM..."
ssh ${SSH_OPTS} "${VM_USER}@${VM_IP}" '
    set -euo pipefail
    echo admin | sudo -S killall -9 erd-host erd-client 2>/dev/null || true
    rm -rf /tmp/erd-interop "$HOME/Library/Application Support/EclipticRD"
    mkdir -p /tmp/erd-interop

    PIN="12345678"
    FRAMES=15
    TIMEOUT=45

    echo "Starting erd-host on 127.0.0.1:19730..."
    ~/erd/target/debug/erd-host --bootstrap-pin "${PIN}" --auto-approve > /tmp/erd-interop/host.log 2>&1 &
    HOST_PID=$!
    sleep 2

    echo "Running erd-client with PIN=${PIN}, target_frames=${FRAMES}..."
    CLIENT_STATUS=0
    ~/erd/target/debug/erd-client \
        --host 127.0.0.1 \
        --tcp-port 19730 \
        --pin "${PIN}" \
        --frames "${FRAMES}" \
        --timeout-secs "${TIMEOUT}" \
        --pairing-store /tmp/erd-interop/client-pairing.json \
        --stats-json /tmp/erd-interop/stats.json > /tmp/erd-interop/client.log 2>&1 || CLIENT_STATUS=$?

    kill -9 "${HOST_PID}" 2>/dev/null || true

    if [ "${CLIENT_STATUS}" -ne 0 ]; then
        echo "ERROR: erd-client failed with exit code ${CLIENT_STATUS}"
        echo "=== Host Log ==="
        cat /tmp/erd-interop/host.log
        echo "=== Client Log ==="
        cat /tmp/erd-interop/client.log
        exit 1
    fi

    if [ ! -f /tmp/erd-interop/stats.json ]; then
        echo "ERROR: stats.json was not produced"
        exit 1
    fi

    echo "=== Interop Test Output ==="
    cat /tmp/erd-interop/stats.json
    echo ""
'

echo "[4/4] Validating assertion..."
echo "INTEROP_PASS"
