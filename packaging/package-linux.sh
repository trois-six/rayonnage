#!/usr/bin/env bash
# Linux archive: the binary (GTK 4 ≥ 4.12 must be installed), a desktop
# entry, the icon and an install script.
set -euo pipefail
VERSION="$1"
NAME="rayonnage-$VERSION-linux-x86_64"
DIST="dist/$NAME"
rm -rf "$DIST" && mkdir -p "$DIST"
cp target/release/rayonnage "$DIST/"
strip "$DIST/rayonnage" || true
cp packaging/rayonnage.desktop "$DIST/org.troissix.Rayonnage.desktop"
cp assets/rayonnage.png "$DIST/org.troissix.Rayonnage.png"
cp README.md LICENSE "$DIST/"
cat > "$DIST/install.sh" <<'SH'
#!/bin/sh
# Installs Rayonnage for the current user (~/.local).
set -e
cd "$(dirname "$0")"
install -Dm755 rayonnage "$HOME/.local/bin/rayonnage"
install -Dm644 org.troissix.Rayonnage.desktop "$HOME/.local/share/applications/org.troissix.Rayonnage.desktop"
install -Dm644 org.troissix.Rayonnage.png "$HOME/.local/share/icons/hicolor/256x256/apps/org.troissix.Rayonnage.png"
echo "Rayonnage installed in ~/.local/bin (make sure it is in your PATH)."
SH
chmod +x "$DIST/install.sh"
tar -C dist -czf "dist/$NAME.tar.gz" "$NAME"
echo "dist/$NAME.tar.gz"
