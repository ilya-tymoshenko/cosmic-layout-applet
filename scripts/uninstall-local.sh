#!/usr/bin/env bash
set -euo pipefail

APPLET_ID="com.ilya.CosmicAppletRealKeyboardLayout"
BIN_NAME="cosmic-layout-applet"

rm -f "$HOME/.local/bin/${BIN_NAME}"
rm -f "$HOME/.local/share/applications/${APPLET_ID}.desktop"

echo "Removed local applet files."
