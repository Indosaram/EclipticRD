#!/usr/bin/env bash
# EclipticRD Linux Binary Installer
# Copies compiled binaries (erd-host, erd-client) to /usr/local/bin and verifies runtime dependencies.

set -euo pipefail

DEST_DIR="${DESTDIR:-/usr/local/bin}"
SOURCE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "=== EclipticRD Linux Installer ==="
echo "Target installation directory: ${DEST_DIR}"

# 1. Dependency Checks (FFmpeg runtime and libraries)
check_ffmpeg() {
    local missing_deps=()

    echo "--> Checking runtime dependencies..."
    if ! command -v ffmpeg >/dev/null 2>&1; then
        missing_deps+=("ffmpeg (CLI)")
    fi

    # Check for core dynamic libraries if ldconfig / pkg-config exists
    local has_pkg_config=0
    if command -v pkg-config >/dev/null 2>&1; then
        has_pkg_config=1
    fi

    for lib in libavcodec libavformat libavutil libswscale libswresample; do
        if [ "$has_pkg_config" -eq 1 ]; then
            if ! pkg-config --exists "$lib" 2>/dev/null; then
                # Check ldconfig cache as fallback
                if ! ldconfig -p 2>/dev/null | grep -q "$lib"; then
                    missing_deps+=("$lib")
                fi
            fi
        else
            if ! ldconfig -p 2>/dev/null | grep -q "$lib"; then
                missing_deps+=("$lib")
            fi
        fi
    done

    if [ ${#missing_deps[@]} -gt 0 ]; then
        echo "WARNING: The following FFmpeg runtime libraries or tools were not detected:"
        for dep in "${missing_deps[@]}"; do
            echo "  - $dep"
        done
        echo ""
        echo "Please install FFmpeg runtime packages using your Linux distribution package manager:"
        echo "  Debian/Ubuntu: sudo apt-get update && sudo apt-get install -y ffmpeg libavcodec-dev libavformat-dev libavutil-dev libswscale-dev libswresample-dev"
        echo "  Fedora/RHEL:   sudo dnf install -y ffmpeg ffmpeg-devel"
        echo "  Arch Linux:    sudo pacman -S --needed ffmpeg"
        echo "  openSUSE:      sudo zypper install ffmpeg ffmpeg-devel"
        echo ""
    else
        echo "--> All FFmpeg runtime dependencies found."
    fi
}

# 2. Binary Installation
install_binary() {
    local bin_name="$1"
    local found_bin=""

    # Look for candidate binaries in standard target paths or current directory
    local candidate_paths=(
        "${SOURCE_DIR}/target/release/${bin_name}"
        "${SOURCE_DIR}/target/x86_64-unknown-linux-gnu/release/${bin_name}"
        "${SOURCE_DIR}/target/debug/${bin_name}"
        "${SOURCE_DIR}/target/x86_64-unknown-linux-gnu/debug/${bin_name}"
        "./${bin_name}"
    )

    for path in "${candidate_paths[@]}"; do
        if [ -f "$path" ] && [ -x "$path" ]; then
            found_bin="$path"
            break
        fi
    done

    if [ -n "$found_bin" ]; then
        echo "--> Installing ${bin_name} from ${found_bin}..."
        install -m 755 -d "${DEST_DIR}"
        install -m 755 "${found_bin}" "${DEST_DIR}/${bin_name}"
        echo "    Installed: ${DEST_DIR}/${bin_name}"
    else
        echo "--> [Notice] Binary '${bin_name}' not found in build targets. Skipping."
    fi
}

check_ffmpeg

# Check for sudo / write permissions
if [ ! -w "${DEST_DIR}" ] && [ "$(id -u)" -ne 0 ]; then
    echo "Need elevated permissions to write to ${DEST_DIR}."
    echo "Re-running installation using sudo..."
    exec sudo bash "$0" "$@"
fi

install_binary "erd-host"
install_binary "erd-client"
install_binary "tauri-shell"

echo "=== Installation finished successfully ==="
