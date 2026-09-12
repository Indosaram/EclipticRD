#!/bin/bash
# MahoRD Development Launcher
# Relauches the app, and optionally resets stale TCC permissions.

BUNDLE_ID="com.indo.MahoRD"
APP_PATH=""

# Find the primary active DerivedData app path
APP_PATH=$(find ~/Library/Developer/Xcode/DerivedData -name 'MahoRD.app' -path '*/Debug/*' -type d 2>/dev/null | head -1)

# Check custom argument
RESET_TCC=false
for arg in "$@"; do
  if [ "$arg" == "--reset" ]; then
    RESET_TCC=true
  fi
done

if [ -z "$APP_PATH" ]; then
  echo "❌ MahoRD.app not found. Build first: xcodebuild -scheme MahoRD build"
  exit 1
fi

if [ "$RESET_TCC" = true ]; then
  echo "🔐 Resetting TCC permissions for $BUNDLE_ID..."
  tccutil reset ScreenCapture "$BUNDLE_ID" 2>/dev/null
  tccutil reset Accessibility "$BUNDLE_ID" 2>/dev/null
  tccutil reset InputMonitoring "$BUNDLE_ID" 2>/dev/null
  echo "✅ TCC reset complete"
else
  echo "ℹ️ Skipping TCC permission reset (use --reset to clear them)"
fi

echo "🚀 Launching $APP_PATH"
open "$APP_PATH"
