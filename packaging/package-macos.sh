#!/usr/bin/env bash
# macOS application: Rayonnage.app with the Homebrew GTK libraries bundled
# (dylibbundler), the GSettings schemas, the icon themes and the image
# loaders, signed ad hoc.
set -euo pipefail
VERSION="$1"
ARCH="$(uname -m)"
APP="dist/Rayonnage.app"
C="$APP/Contents"
RES="$C/Resources"
BREW="$(brew --prefix)"
rm -rf "$APP" && mkdir -p "$C/MacOS" "$RES" "$C/libs"

cp target/release/rayonnage "$C/MacOS/rayonnage-bin"
dylibbundler -od -b -cd -x "$C/MacOS/rayonnage-bin" -d "$C/libs" -p @executable_path/../libs > /dev/null

# Image loaders (SVG icons...); their cache is written at launch with the
# real location of the application.
LOADERS=lib/gdk-pixbuf-2.0/2.10.0/loaders
mkdir -p "$RES/$LOADERS"
shopt -s nullglob
cp "$BREW/$LOADERS"/*.so "$RES/$LOADERS/"
for l in "$RES/$LOADERS"/*.so; do
    dylibbundler -of -b -cd -x "$l" -d "$C/libs" -p @executable_path/../libs > /dev/null
done
GDK_PIXBUF_MODULEDIR="$BREW/$LOADERS" gdk-pixbuf-query-loaders \
    | sed "s|$BREW/$LOADERS|@RES@/$LOADERS|g" > "$RES/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache.in"

mkdir -p "$RES/share/glib-2.0/schemas" "$RES/share/icons"
cp "$BREW"/share/glib-2.0/schemas/*.xml "$RES/share/glib-2.0/schemas/"
glib-compile-schemas "$RES/share/glib-2.0/schemas"
cp -RL "$BREW/share/icons/Adwaita" "$BREW/share/icons/hicolor" "$RES/share/icons/"

cat > "$C/MacOS/rayonnage" <<'SH'
#!/bin/sh
HERE="$(cd "$(dirname "$0")" && pwd)"
RES="$(cd "$HERE/../Resources" && pwd)"
export XDG_DATA_DIRS="$RES/share"
export GSETTINGS_SCHEMA_DIR="$RES/share/glib-2.0/schemas"
CACHE="${TMPDIR:-/tmp}/rayonnage-pixbuf-loaders.cache"
sed "s|@RES@|$RES|g" "$RES/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache.in" > "$CACHE"
export GDK_PIXBUF_MODULE_FILE="$CACHE"
exec "$HERE/rayonnage-bin" "$@"
SH
chmod +x "$C/MacOS/rayonnage"

# Icon.
ICONSET="dist/rayonnage.iconset"
rm -rf "$ICONSET" && mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
    sips -z $s $s packaging/icon-1024.png --out "$ICONSET/icon_${s}x${s}.png" > /dev/null
    sips -z $((s * 2)) $((s * 2)) packaging/icon-1024.png --out "$ICONSET/icon_${s}x${s}@2x.png" > /dev/null
done
iconutil -c icns "$ICONSET" -o "$RES/rayonnage.icns"

sed "s/@VERSION@/$VERSION/g" packaging/Info.plist > "$C/Info.plist"
# Every Mach-O file must carry a signature on Apple silicon: sign them all
# (ad hoc), then the bundle.
find "$C/libs" "$RES/lib" -type f \( -name '*.dylib' -o -name '*.so' \) -exec codesign --force --sign - {} \;
codesign --force --sign - "$C/MacOS/rayonnage-bin"
codesign --force --sign - "$APP"

NAME="Rayonnage-$VERSION-macos-$ARCH"
ditto -c -k --keepParent "$APP" "dist/$NAME.zip"
echo "dist/$NAME.zip"
