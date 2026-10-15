#!/bin/sh
# Builds TypeThru.app and wraps it in TypeThru-<version>.dmg for download.
# With a Developer ID certificate and TYPETHRU_NOTARY_PROFILE (a notarytool keychain profile),
# the disk image is also notarized, so it opens without a Gatekeeper warning.
set -e
cd "$(dirname "$0")/.."
scripts/build.sh
VERSION=$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' TypeThru.app/Contents/Info.plist)
DMG="TypeThru-$VERSION.dmg"
VOLUME=/Volumes/TypeThru
if [ -e "$VOLUME" ]; then echo "Eject the TypeThru disk first ($VOLUME)."; exit 1; fi
# DECISION: a fresh staging folder in TMPDIR, which macOS cleans, instead of deleting folders here.
STAGE=$(mktemp -d "${TMPDIR:-/tmp}/typethru-dmg.XXXXXX")
rm -f "$DMG"
ditto TypeThru.app "$STAGE/TypeThru.app"
ln -s /Applications "$STAGE/Applications"
# The window background, drawn by scripts/dmg-background.swift at 1x and 2x.
TOOL=$(mktemp -d "${TMPDIR:-/tmp}/typethru-background.XXXXXX")
cp scripts/dmg-background.swift "$TOOL/main.swift"
swiftc -module-cache-path "$TOOL/module-cache" "$TOOL/main.swift" Sources/Brand.swift -o "$TOOL/background"
"$TOOL/background" "$TOOL"
mkdir "$STAGE/.background"
tiffutil -cathidpicheck "$TOOL/background.png" "$TOOL/background@2x.png" -out "$STAGE/.background/background.tiff" 2>/dev/null

# Lay out the window in Finder on a writable copy, then compress it.
hdiutil create -quiet -volname TypeThru -srcfolder "$STAGE" -format UDRW -fs HFS+ -size 64m "$STAGE.rw.dmg"
hdiutil attach -quiet -readwrite -noverify -noautoopen "$STAGE.rw.dmg"
trap 'hdiutil detach -quiet "$VOLUME" 2>/dev/null || true; rm -f "$STAGE.rw.dmg"' EXIT
# DECISION: Finder writes the window layout to .DS_Store; macOS may ask once to let this terminal control Finder.
osascript <<'APPLESCRIPT' || { echo 'Finder did not answer. Unlock the Mac, allow this terminal to control Finder (System Settings > Privacy & Security > Automation), and run make package again.'; exit 1; }
tell application "Finder"
    tell disk "TypeThru"
        open
        set current view of container window to icon view
        set toolbar visible of container window to false
        set statusbar visible of container window to false
        -- 680 x 420 points of content, centred on a 1512 x 982 laptop screen.
        set the bounds of container window to {416, 267, 1096, 715}
        set options to the icon view options of container window
        set arrangement of options to not arranged
        set icon size of options to 128
        set text size of options to 14
        set background picture of options to file ".background:background.tiff"
        set position of item "TypeThru.app" of container window to {190, 215}
        set position of item "Applications" of container window to {490, 215}
        -- Again last: Finder may widen the window while it applies the options above.
        set the bounds of container window to {416, 267, 1096, 715}
        update without registering applications
        delay 1
        close
    end tell
end tell
APPLESCRIPT
# DECISION: after Finder, which removes a volume icon it finds while setting up the window.
cp Resources/AppIcon.icns "$VOLUME/.VolumeIcon.icns"
# C: use the volume icon. E: show "TypeThru" without ".app".
{ SetFile -a C "$VOLUME" && SetFile -a E "$VOLUME/TypeThru.app"; } 2>/dev/null ||
    echo 'Note: SetFile not found (Xcode command line tools), so the disk keeps the plain icon and the .app name.'
sync
hdiutil detach -quiet "$VOLUME"
hdiutil convert -quiet "$STAGE.rw.dmg" -format UDZO -imagekey zlib-level=9 -o "$DMG"
if [ -n "${TYPETHRU_NOTARY_PROFILE-}" ]; then
    xcrun notarytool submit "$DMG" --keychain-profile "$TYPETHRU_NOTARY_PROFILE" --wait
    xcrun stapler staple "$DMG"
fi
# The app installs an update only with a valid signature from the key in Sources/UpdateKey.swift.
swift scripts/update-key.swift sign "$DMG" > "$DMG.sig" || { rm -f "$DMG.sig"; exit 1; }
codesign -dr - TypeThru.app 2>&1 | grep -q 'cdhash' &&
    echo 'Warning: signed ad-hoc. Users who update in the app must allow Accessibility again. Run make cert first.'
shasum -a 256 "$DMG"
echo "Packaged $(pwd)/$DMG and $DMG.sig. Attach both to the GitHub release."
