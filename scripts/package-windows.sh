#!/bin/sh
# Downloads the Windows exe that the Windows workflow built from the current commit and signs it
# for in-app updates. Only a Windows build writes the exe's icon, version, and manifest, so the
# exe is never built on the Mac. Needs the gh CLI, signed in, and the update key in the keychain.
set -eu
cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\([^"]*\)"/\1/p' windows/Cargo.toml | head -n 1)
VERSION=${VERSION%.0} # 0.7.0 is released as 0.7
grep -q "^## $VERSION " CHANGELOG.md || { echo "Add a '## $VERSION ·' entry to CHANGELOG.md first."; exit 1; }
[ -z "$(git status --porcelain -- windows CHANGELOG.md shared)" ] || { echo "Commit and push the Windows changes first."; exit 1; }
COMMIT=$(git rev-parse HEAD)
RUN=$(gh run list --workflow Windows --commit "$COMMIT" --status success --json databaseId -q '.[0].databaseId')
[ -n "$RUN" ] || { echo "No successful Windows build of $COMMIT yet. Push, wait for the Windows workflow, then run this again."; exit 1; }
EXE="TypeThru-$VERSION-windows.exe"
WORK=$(mktemp -d "${TMPDIR:-/tmp}/typethru-windows.XXXXXX")
gh run download "$RUN" --name "TypeThru-rust-windows-$COMMIT" --dir "$WORK"
mv "$WORK/TypeThru.exe" "$EXE"
rmdir "$WORK"
# The app installs an update only with a valid signature from the key in Sources/UpdateKey.swift.
swift scripts/update-key.swift sign "$EXE" > "$EXE.sig" || { rm -f "$EXE.sig"; exit 1; }
shasum -a 256 "$EXE"
echo "Packaged $(pwd)/$EXE and $EXE.sig from Windows build $RUN. Attach both to the GitHub release."
