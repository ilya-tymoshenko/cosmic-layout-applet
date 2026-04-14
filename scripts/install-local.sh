#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APPLET_ID="com.ilya.CosmicAppletRealKeyboardLayout"
BIN_NAME="cosmic-layout-applet"

source "$HOME/.cargo/env"

cd "$PROJECT_DIR"
cargo build --release

install -Dm0755 "target/release/${BIN_NAME}" "$HOME/.local/bin/${BIN_NAME}"
install -Dm0644 "data/${APPLET_ID}.desktop" "$HOME/.local/share/applications/${APPLET_ID}.desktop"

echo "Installed:"
echo "  ~/.local/bin/${BIN_NAME}"
echo "  ~/.local/share/applications/${APPLET_ID}.desktop"
echo
echo "Next:"
echo "  1) Add applet id '${APPLET_ID}' into panel plugins list"
echo "  2) Restart panel: pkill cosmic-panel && nohup cosmic-panel >/tmp/cosmic-panel.log 2>&1 &"
