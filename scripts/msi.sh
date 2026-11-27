#!/usr/bin/env bash
# Dostoevsky for Windows as an .msi (dsky-fbso.14): packaging/windows/dostoevsky.wxs says what
# it installs. Run in Git Bash on Windows (CI: .github/workflows/ci.yml, release.yml).
#
#   scripts/msi.sh              # target/bundle/Dostoevsky-<version>-x64.msi
#   UPDATES=1 scripts/msi.sh    # with the update check (updates.rs): the project's releases only
#
# Needs WiX 5 (dotnet tool install --global wix --version 5.0.2) and cargo-about for the
# third-party licenses (cargo install cargo-about). Not signed yet: Windows shows its unknown
# publisher warning until a code-signing certificate is set up.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
OUT=target/bundle
WORK=target/msi
MSI="$OUT/Dostoevsky-$VERSION-x64.msi"

command -v wix >/dev/null || { echo "WiX is needed: dotnet tool install --global wix --version 5.0.2" >&2; exit 1; }
command -v cargo-about >/dev/null || { echo "cargo-about is needed for a release build (cargo install cargo-about)" >&2; exit 1; }

FEATURES=""
[ "${UPDATES:-}" = 1 ] && FEATURES=updates
cargo build --release --locked -p dostoevsky ${FEATURES:+--features "$FEATURES"}

rm -rf "$WORK"
mkdir -p "$WORK/licenses" "$OUT"
cp LICENSE NOTICE crates/app/assets/fonts/OFL-*.txt "$WORK/licenses/"
# Apache/MIT/BSD ask for their notices to travel with the binary (dsky-a8n.3).
cargo about generate --frozen packaging/about.hbs -o "$WORK/licenses/THIRD-PARTY-LICENSES.txt"

rm -f "$MSI"
wix build packaging/windows/dostoevsky.wxs -arch x64 \
  -d Version="$VERSION" \
  -d Exe=target/release/dostoevsky.exe \
  -d Icon=packaging/windows/dostoevsky.ico \
  -d Licenses="$WORK/licenses" \
  -o "$MSI"
echo "$MSI"
