#!/usr/bin/env bash
# Build Carbon in release mode and install it for the current user.
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

cargo build --release
install -Dm755 target/release/carbon "$HOME/.local/bin/carbon"
install -Dm644 assets/carbon.desktop "$HOME/.local/share/applications/carbon.desktop"
install -Dm644 assets/carbon.svg "$HOME/.local/share/icons/hicolor/scalable/apps/carbon.svg"
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true

echo "Installed: ~/.local/bin/carbon"
echo "Hyprland: see assets/hyprland-carbon.lua for the opacity rule that keeps text un-dimmed."
