#!/bin/sh
# Security and repository audit. Read-only: changes nothing outside .build and posts no input.
# Run by make audit and make check (before every push). Needs: brew install shellcheck gitleaks
set -eu
cd "$(dirname "$0")/.."
APP=${TYPETHRU_APP:-TypeThru.app}
failed=0
fail() { echo "FAIL: $*"; failed=1; }
ok() { echo "ok  $*"; }
need() { command -v "$1" >/dev/null 2>&1 || { fail "$1 is not installed. Run: brew install $1"; return 1; }; }

# Commits not yet pushed: compared with the upstream branch, or everything if there is none.
UPSTREAM=$(git rev-parse --verify -q '@{upstream}' 2>/dev/null || git rev-parse --verify -q origin/main || true)
RANGE=${UPSTREAM:+$UPSTREAM..HEAD}

# 1. Static analysis of the root helper.
SOURCE=.build/virtualhid-source
if [ -d "$SOURCE/include" ]; then
    # DECISION: unix.BlockInCriticalSection is off. It reads std::weak_ptr::lock() in the driver
    # library as a mutex and reports the helper's recv; no mutex is held there.
    found=$(clang++ --analyze -std=c++23 -isystem "$SOURCE/include" -isystem "$SOURCE/vendor/vendor/include" \
        -Xanalyzer -analyzer-disable-checker=unix.BlockInCriticalSection \
        -I helper -o /dev/null helper/virtual-keyboard.cpp 2>&1 | grep -E 'warning:|error:' \
        | grep -v 'Path diagnostic report is not generated' || true)
    [ -z "$found" ] && ok "clang static analyzer: root helper" || fail "clang static analyzer:
$found"
else
    fail "driver headers missing; run make build first"
fi

# 2. Shell scripts.
if need shellcheck; then
    if shellcheck -S warning scripts/*.sh helper/*.sh .githooks/pre-push; then ok "shellcheck: all scripts"
    else fail "shellcheck found problems above"; fi
fi

# 3. Secrets in the whole git history.
if need gitleaks; then
    if gitleaks git --no-banner --redact --log-level error .; then ok "gitleaks: no secrets in git history"
    else fail "gitleaks found possible secrets above"; fi
fi

# 4. No personal email addresses in commits about to be pushed.
emails=$(git log --format='%ae%n%ce' ${RANGE:+"$RANGE"} | sort -u | grep -v '@users\.noreply\.github\.com$' || true)
[ -z "$emails" ] && ok "commit emails are GitHub noreply addresses" || fail "personal email in commits to push: $emails"

# 5. Repository hygiene.
bad=$(git ls-files | grep -E '(^|/)(\.DS_Store|DEVLOG\.md|ResourceHashes\.swift)$|\.app/|\.dmg$|\.dmg\.sig$|^\.build/|\.iconset/' || true)
[ -z "$bad" ] && ok "no build output or local files tracked" || fail "tracked files that belong local only: $bad"
ignored=$(git ls-files -ci --exclude-standard)
[ -z "$ignored" ] && ok "no tracked file is also ignored" || fail "tracked but ignored: $ignored"
large=$(git ls-files -z | xargs -0 stat -f '%z %N' | awk '$1 > 2000000 { print $2 }')
[ -z "$large" ] && ok "no tracked file over 2 MB" || fail "large tracked files: $large"
untracked=$(git ls-files --others --exclude-standard)
[ -z "$untracked" ] || echo "note: untracked files (add or ignore them): $untracked"
# Commands only: comments and this check's own pattern do not count.
removals=$(grep -n -E '^[^#]*rm -[a-zA-Z]*r[a-zA-Z]*f' scripts/*.sh helper/*.sh .githooks/pre-push | grep -v '^scripts/audit.sh:' || true)
[ -z "$removals" ] && ok "no rm -rf in scripts" || fail "rm -rf in scripts (delete specific files instead): $removals"

# Release notes: the app shows CHANGELOG.md after an update, so the version being built needs an entry.
version=$(sed -n 's|.*<key>CFBundleShortVersionString</key><string>\([^<]*\)</string>.*|\1|p' scripts/build.sh)
grep -q "^## $version " CHANGELOG.md && ok "CHANGELOG.md has notes for $version" || fail "CHANGELOG.md has no '## $version ·' entry"

# 6. Privacy contract (README "Privacy"): no event taps, input monitoring, or logging; clipboard
# writes and screen capture only in Copy Text from Screen; network only for the update check.
taps=$(grep -n -E 'tapCreate|addGlobalMonitorForEvents|IOHIDManagerOpen|IOHIDDeviceOpen|NSLog\(|os_log|Logger\(|OSLog' Sources/*.swift || true)
[ -z "$taps" ] && ok "no event taps or logging" || fail "breaks the privacy contract: $taps"
screen=$(grep -n -E 'setString\(|clearContents\(|screencapture|SCScreenshotManager|SCStream|CGWindowListCreateImage|CGDisplayCreateImage' Sources/*.swift | grep -v '^Sources/ScreenText.swift:' || true)
[ -z "$screen" ] && ok "clipboard writes and screen capture only in Copy Text from Screen" || fail "clipboard write or screen capture outside ScreenText.swift: $screen"
network=$(grep -n -E 'URLSession|URLRequest|NWConnection|NWPathMonitor|CFStream|getaddrinfo' Sources/*.swift | grep -v '^Sources/Updates.swift:' || true)
[ -z "$network" ] && ok "network access only in the update check" || fail "network access outside Updates.swift: $network"
# The Windows app keeps the same contract: no keyboard hooks or raw input, no logging, clipboard
# writes and screen capture only in Copy Text from Screen, network only in the update check.
wintaps=$(grep -rn -E 'SetWindowsHookEx|RegisterRawInputDevices|GetRawInputData|WH_KEYBOARD|Debug\.Write|Trace\.Write|EventLog|ILogger|Console\.Write' windows --include='*.cs' | grep -v '/bin/\|/obj/' || true)
[ -z "$wintaps" ] && ok "Windows: no keyboard hooks, raw input, or logging" || fail "Windows app breaks the privacy contract: $wintaps"
winscreen=$(grep -rn -E 'Clipboard\.(SetText|SetData|SetDataObject)|[gG]raphics\.CopyFromScreen|BitBlt|PrintWindow' windows --include='*.cs' | grep -v '/bin/\|/obj/\|windows/TypeThru/ScreenCopy.cs:' || true)
[ -z "$winscreen" ] && ok "Windows: clipboard writes and screen capture only in ScreenCopy.cs" || fail "Windows clipboard write or screen capture outside ScreenCopy.cs: $winscreen"
winnet=$(grep -rn -E 'HttpClient|WebRequest|WebClient|TcpClient|Sockets' windows --include='*.cs' | grep -v '/bin/\|/obj/\|windows/TypeThru/Updater.cs:' || true)
[ -z "$winnet" ] && ok "Windows: network access only in Updater.cs" || fail "Windows network access outside Updater.cs: $winnet"
# The Rust version of the Windows app (replacing the C# one) keeps the same contract, and its
# unsafe code (direct Windows calls) stays in the files that need it, so it is easy to review.
rs() { grep -rn -E "$1" windows --include='*.rs' --include='*.slint' | grep -v '^windows/target/' || true; }
rtaps=$(rs 'SetWindowsHookEx|RegisterRawInputDevices|GetRawInputData|WH_KEYBOARD|(print|println|dbg)!\(|(^|[^a-z_])(log|tracing)::' | grep -v '^windows/[a-z]*/build.rs:' || true)
[ -z "$rtaps" ] && ok "Windows (Rust): no keyboard hooks, raw input, or logging" || fail "Windows app (Rust) breaks the privacy contract: $rtaps"
rscreen=$(rs 'SetClipboardData|EmptyClipboard|BitBlt|StretchBlt|PrintWindow|GetDIBits' | grep -v '^windows/app/src/screen_copy.rs:' || true)
[ -z "$rscreen" ] && ok "Windows (Rust): clipboard writes and screen capture only in screen_copy.rs" || fail "Windows (Rust) clipboard write or screen capture outside screen_copy.rs: $rscreen"
rnet=$(rs 'WinHttp|std::net|TcpStream|UdpSocket|reqwest|ureq|hyper::' | grep -v '^windows/app/src/updater.rs:' || true)
[ -z "$rnet" ] && ok "Windows (Rust): network access only in updater.rs" || fail "Windows (Rust) network access outside updater.rs: $rnet"
runsafe=$(rs '(^|[^a-z_])unsafe([^a-z_]|$)' | grep -v -E '^windows/app/src/(native|screen_copy|install)\.rs:|forbid\(unsafe_code\)|unsafe_op_in_unsafe_fn' || true)
[ -z "$runsafe" ] && ok "Windows (Rust): unsafe code only in native.rs, screen_copy.rs, and install.rs" || fail "Windows (Rust) unsafe code elsewhere: $runsafe"
mackey=$(sed -n 's/.*base64Encoded: "\([^"]*\)".*/\1/p' Sources/UpdateKey.swift)
winkey=$(sed -n 's/.*PublicKey = Convert.FromBase64String("\([^"]*\)").*/\1/p' windows/TypeThru.Core/UpdateFeed.cs)
rustkey=$(sed -n 's/.*PUBLIC_KEY: &str = "\([^"]*\)".*/\1/p' windows/core/src/update_feed.rs)
[ -n "$mackey" ] && [ "$mackey" = "$winkey" ] && [ "$mackey" = "$rustkey" ] && ok "every app checks updates with the same public key" || fail "update public key differs between Sources/UpdateKey.swift, windows/TypeThru.Core/UpdateFeed.cs, and windows/core/src/update_feed.rs"

prints=$(grep -n 'print(' Sources/*.swift | grep -v '^Sources/main.swift:' || true)
[ -z "$prints" ] && ok "no print output outside --print-map" || fail "print outside main.swift: $prints"
if [ -n "$RANGE" ] && git diff "$RANGE" -- '*.swift' '*.cpp' '*.hpp' ':!Tests' | grep -E '^\+' | grep -q -E 'URLSession|NWConnection|tapCreate|addGlobalMonitor|NSPasteboard|AXUIElement|CGEventSource\.(keyState|buttonState)|UserDefaults'; then
    if git diff --quiet "$RANGE" -- README.md; then
        fail "changes touch clipboard, network, settings, or input checks; review README Privacy and update it in the same push"
    else
        ok "privacy-related changes come with a README update (review it)"
    fi
else
    ok "no new privacy-related code to review"
fi

# 7. No VDI or remote desktop vendor names in the app or README.
vendor_names='citrix|vmware|horizon client|omnissa|workspaces|azure virtual desktop|windows 365|parallels|teamviewer|anydesk|chrome remote'
vendors=$( { grep -n -i -E "$vendor_names" Sources/*.swift README.md windows/TypeThru/*.cs windows/TypeThru.Core/*.cs; grep -rn -i -E "$vendor_names" windows --include='*.rs' --include='*.slint' | grep -v '^windows/target/'; } || true)
[ -z "$vendors" ] && ok "no vendor names in the app or README" || fail "vendor names: $vendors"

# 8. Built app: valid signature, and setup fingerprints match the bundled files.
if [ -d "$APP" ]; then
    codesign --verify --deep --strict "$APP" 2>/dev/null && ok "$APP signature valid" || fail "$APP signature invalid"
    for file in install-helper.sh uninstall-helper.sh TypeThru-VirtualKeyboard Karabiner-DriverKit-VirtualHIDDevice-8.6.0.pkg; do
        hash=$(shasum -a 256 "$APP/Contents/Resources/$file" | cut -c1-64)
        grep -q "\"$file\": \"$hash\"" .build/ResourceHashes.swift || fail "setup fingerprint mismatch for $file; rebuild"
    done
    ok "setup fingerprints match the bundled files"
fi

# 9. Installed system, if set up (read-only).
HELPER=/Library/PrivilegedHelperTools/TypeThru-VirtualKeyboard
PLIST=/Library/LaunchDaemons/io.github.sayre4ux.typethru.virtual-keyboard.plist
SOCKET=/var/run/io.github.sayre4ux.typethru.virtual-keyboard.sock
if [ -e "$HELPER" ]; then
    [ "$(stat -f '%Su:%Sg %Lp' "$HELPER")" = "root:wheel 755" ] && ok "installed helper owned by root, mode 755" || fail "installed helper owner or mode: $(stat -f '%Su:%Sg %Lp' "$HELPER")"
    [ "$(stat -f '%Su:%Sg %Lp' "$PLIST")" = "root:wheel 644" ] && ok "launch daemon plist owned by root, mode 644" || fail "plist owner or mode: $(stat -f '%Su:%Sg %Lp' "$PLIST")"
    if [ -S "$SOCKET" ]; then
        [ "$(stat -f '%Su %Lp' "$SOCKET")" = "$(id -un) 600" ] && ok "helper socket only for $(id -un), mode 600" || fail "socket owner or mode: $(stat -f '%Su %Lp' "$SOCKET")"
    fi
else
    echo "note: Virtual Keyboard is not installed on this Mac; skipped installed-file checks"
fi

[ "$failed" = 0 ] && echo "Audit passed." || { echo "Audit failed."; exit 1; }
