#!/bin/sh
# Windows app checks that also run on a Mac: the shared core's tests and a compile of the app.
# Needs the .NET 10 SDK; without it, the Windows workflow on GitHub runs them instead.
set -eu
cd "$(dirname "$0")/../windows"
DOTNET=$(command -v dotnet || true)
[ -x "$HOME/.dotnet/dotnet" ] && DOTNET="$HOME/.dotnet/dotnet"
if [ -z "$DOTNET" ] || ! "$DOTNET" --list-sdks | grep -q '^10\.'; then
    echo "note: no .NET 10 SDK, so the Windows checks run only on GitHub (Windows workflow)."
    exit 0
fi
export DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1
"$DOTNET" test TypeThru.Tests -c Release -v quiet
"$DOTNET" build TypeThru/TypeThru.csproj -c Release -v quiet
echo "Windows core tests passed and the Windows app compiles."
