#!/bin/sh
# Windows app checks that also run on a Mac. The Rust app (windows/Cargo.toml): format, lint,
# tests, and a compile for Windows; rustup installs the pinned toolchain beside the default one.
# The C# app (being replaced): its core tests and a compile, when the .NET 10 SDK is installed.
# Without a tool, the Windows workflow on GitHub runs those checks instead.
set -eu
cd "$(dirname "$0")/../windows"
if command -v rustup >/dev/null 2>&1; then
    rustup toolchain install >/dev/null # the version in rust-toolchain.toml
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo test --workspace --locked --quiet
    cargo clippy --workspace --all-targets --locked --target x86_64-pc-windows-msvc -- -D warnings
    echo "Windows (Rust): core tests passed; the code is formatted, lint-free, and compiles for Windows."
else
    echo "note: no rustup, so the Rust checks run only on GitHub (Windows workflow)."
fi
DOTNET=$(command -v dotnet || true)
[ -x "$HOME/.dotnet/dotnet" ] && DOTNET="$HOME/.dotnet/dotnet"
if [ -z "$DOTNET" ] || ! "$DOTNET" --list-sdks | grep -q '^10\.'; then
    echo "note: no .NET 10 SDK, so the C# checks run only on GitHub (Windows workflow)."
    exit 0
fi
export DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1
"$DOTNET" test TypeThru.Tests -c Release -v quiet
"$DOTNET" build TypeThru/TypeThru.csproj -c Release -v quiet
echo "Windows (C#): core tests passed and the app compiles."
