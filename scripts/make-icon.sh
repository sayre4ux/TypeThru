#!/bin/sh
# Regenerates Resources/AppIcon.icns and the README logos in docs/ from scripts/make-icon.swift and Sources/Brand.swift.
set -e
cd "$(dirname "$0")/.."
WORK=$(mktemp -d "${TMPDIR:-/tmp}/typethru-icon.XXXXXX")
# Top-level code must live in a file named main.swift.
cp scripts/make-icon.swift "$WORK/main.swift"
swiftc -module-cache-path "$WORK/module-cache" "$WORK/main.swift" Sources/Brand.swift -o "$WORK/make-icon"
"$WORK/make-icon" "$WORK/AppIcon.iconset" docs
iconutil -c icns "$WORK/AppIcon.iconset" -o Resources/AppIcon.icns
# The Windows app icon from the same drawings.
for size in 16 32 48 64 128 256; do sips -z "$size" "$size" "$WORK/AppIcon.iconset/icon_512x512.png" --out "$WORK/win-$size.png" >/dev/null; done
python3 scripts/make-ico.py windows/TypeThru/TypeThru.ico "$WORK"/win-16.png "$WORK"/win-32.png "$WORK"/win-48.png "$WORK"/win-64.png "$WORK"/win-128.png "$WORK"/win-256.png
echo "Wrote Resources/AppIcon.icns, windows/TypeThru/TypeThru.ico, and docs/logo-*.png"
