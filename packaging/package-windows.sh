#!/usr/bin/env bash
# Windows archive (run in MSYS2 UCRT64): the executable with every GTK DLL
# it needs, the GSettings schemas, the icon themes and the image loaders,
# laid out as GTK expects (bin/, lib/, share/).
set -euo pipefail
VERSION="$1"
NAME="rayonnage-$VERSION-windows-x86_64"
DIST="dist/$NAME"
PREFIX=/ucrt64
rm -rf "$DIST" && mkdir -p "$DIST/bin" "$DIST/lib" "$DIST/share/glib-2.0/schemas" "$DIST/share/icons"
cp target/release/rayonnage.exe "$DIST/bin/"

# DLLs of the MSYS2 prefix used by a file, recursively.
copy_deps() {
    ldd "$1" | awk -v p="$PREFIX/" 'index($3, p) == 1 { print $3 }' | while read -r dll; do
        if [ ! -f "$DIST/bin/$(basename "$dll")" ]; then
            cp "$dll" "$DIST/bin/"
            copy_deps "$dll"
        fi
    done
}
copy_deps "$DIST/bin/rayonnage.exe"

# Image loaders (SVG icons...), with a cache using paths relative to the prefix.
LOADERS=lib/gdk-pixbuf-2.0/2.10.0/loaders
mkdir -p "$DIST/$LOADERS"
cp "$PREFIX/$LOADERS"/*.dll "$DIST/$LOADERS/"
for l in "$DIST/$LOADERS"/*.dll; do copy_deps "$l"; done
GDK_PIXBUF_MODULEDIR="$PREFIX/$LOADERS" gdk-pixbuf-query-loaders | tr "\\\\" / \
    | sed -e "s|$(cygpath -m "$PREFIX")/||g" -e "s|$PREFIX/||g" > "$DIST/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache"

grep -i "\.dll" "$DIST/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache" || true
cp "$PREFIX"/share/glib-2.0/schemas/*.xml "$DIST/share/glib-2.0/schemas/"
glib-compile-schemas "$DIST/share/glib-2.0/schemas"
cp -r "$PREFIX/share/icons/Adwaita" "$PREFIX/share/icons/hicolor" "$DIST/share/icons/"
cp README.md LICENSE "$DIST/"
(cd dist && zip -qr "$NAME.zip" "$NAME")
echo "dist/$NAME.zip"
