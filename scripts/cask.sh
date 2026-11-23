#!/usr/bin/env bash
# Prints the Homebrew cask (packaging/homebrew/dostoevsky.rb) for a release .dmg (dsky-a8n.23).
#
#   scripts/cask.sh [Dostoevsky-X.Y.Z.dmg]   # prints the cask
#   scripts/cask.sh --release vX.Y.Z         # downloads that GitHub Release's .dmg to target/cask/
#
# Without a .dmg it takes target/bundle/Dostoevsky-<Cargo.toml version>.dmg. The checksum has to be
# of the very file on the release, so pass the .dmg you uploaded (or --release). Every local file
# this script reads or writes stays inside the repository.
set -euo pipefail
cd "$(dirname "$0")/.."

DMG="" RELEASE=""
while [ $# -gt 0 ]; do
  case "$1" in
    --release) RELEASE="$2"; shift 2 ;;
    -h|--help) sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) DMG="$1"; shift ;;
  esac
done

if [ -n "$RELEASE" ]; then
  [[ $RELEASE =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "release must look like vX.Y.Z" >&2; exit 1; }
  TMP="$PWD/target/cask/$RELEASE"
  rm -rf "$TMP"; mkdir -p "$TMP"
  gh release download "$RELEASE" -R meroving/dostoevsky --pattern 'Dostoevsky-*.dmg' --dir "$TMP"
  DMG="$(ls "$TMP"/Dostoevsky-*.dmg | head -1)"
fi
if [ -z "$DMG" ]; then
  DMG="target/bundle/Dostoevsky-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1).dmg"
fi
case "$DMG" in
  /*|..|../*|*/..|*/../*) echo "the .dmg must stay inside the repository" >&2; exit 1 ;;
esac
[ -f "$DMG" ] || { echo "no such .dmg: $DMG" >&2; exit 1; }
DMG="$(cd "$(dirname "$DMG")" && pwd -P)/$(basename "$DMG")"
case "$DMG" in
  "$PWD"/*) ;;
  *) echo "the .dmg must be inside the repository" >&2; exit 1 ;;
esac

VERSION="$(basename "$DMG" | sed -n 's/^Dostoevsky-\(.*\)\.dmg$/\1/p')"
[ -n "$VERSION" ] || { echo "can't read the version from $(basename "$DMG") (want Dostoevsky-X.Y.Z.dmg)" >&2; exit 1; }
SHA="$(shasum -a 256 "$DMG" | cut -d' ' -f1)"
sed -e "s/__VERSION__/$VERSION/" -e "s/__SHA256__/$SHA/" -e "1,/^cask /{/^#/d;}" packaging/homebrew/dostoevsky.rb
