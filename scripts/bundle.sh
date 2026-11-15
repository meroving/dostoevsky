#!/usr/bin/env bash
# Builds Dostoevsky.app (universal: arm64 + x86_64), signs it, optionally notarizes it, and
# wraps it in a .dmg (PRD NFR-7).
#
#   scripts/bundle.sh                      # ad-hoc signed, for local testing
#   SIGN_IDENTITY="Developer ID Application: Name (TEAMID)" \
#   NOTARY_PROFILE=dostoevsky-notary scripts/bundle.sh   # release
#
# NOTARY_PROFILE is a keychain profile created once with
#   xcrun notarytool store-credentials dostoevsky-notary --apple-id … --team-id … --password …
# or notarize with an Apple API key instead (what CI uses):
#   NOTARY_KEY_ID=… NOTARY_ISSUER=… scripts/bundle.sh
# Put the key at target/secrets/AuthKey_<NOTARY_KEY_ID>.p8; target/ is ignored.
# ARCHS="aarch64" builds one architecture only.
#
# The .dmg build signed by the project's team checks for updates (the `updates` feature,
# crates/app/src/updates.rs; see UPDATES_TEAM below).
#
# For tests (scripts/e2e_bundle.sh): FEATURES=shots builds the scripted driver in,
# BUNDLE_ID gives the app another identifier (its own recent files),
# OUT puts the bundle elsewhere inside this checkout, NO_DMG=1 stops after signing the .app.
set -euo pipefail
cd "$(dirname "$0")/.."

ARCHS="${ARCHS:-aarch64 x86_64}"
# Only a build signed by the project's own team checks dostoevsky.app for updates (updates.rs): a
# fork signed by anyone else, or an unsigned build, asks nobody, so its writers aren't sent to this
# project's releases. A fork with its own feed points URL in crates/app/src/updates.rs at it and
# puts its team ID here.
UPDATES_TEAM=PLT967TAS9
case "${SIGN_IDENTITY:-}" in
  *"($UPDATES_TEAM)") FEATURES="updates${FEATURES:+,$FEATURES}" ;;
esac
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
# Build numbers count the history that came before this repository's, so they only grow.
BUILD="$((359 + $(git rev-list --count HEAD 2>/dev/null || echo 1)))"
OUT="${OUT:-target/bundle}"
case "$OUT" in
  target|target/*) ;;
  *) echo "OUT must be below target/" >&2; exit 1 ;;
esac
case "/$OUT/" in
  */../*|*/./*) echo "OUT must not contain . or .." >&2; exit 1 ;;
esac
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd -P)"
case "$OUT" in
  "$PWD"/*) ;;
  *) echo "OUT must be inside the repository" >&2; exit 1 ;;
esac
APP="$OUT/Dostoevsky.app"

bins=()
for arch in $ARCHS; do
  triple="$arch-apple-darwin"
  rustup target add "$triple" >/dev/null 2>&1 || true
  MACOSX_DEPLOYMENT_TARGET=13.0 cargo build --release -p dostoevsky --target "$triple" ${FEATURES:+--features "$FEATURES"}
  bins+=("target/$triple/release/dostoevsky")
done

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
if [ "${#bins[@]}" -gt 1 ]; then
  lipo -create -output "$APP/Contents/MacOS/dostoevsky" "${bins[@]}"
else
  cp "${bins[0]}" "$APP/Contents/MacOS/dostoevsky"
fi
sed -e "s/__VERSION__/$VERSION/" -e "s/__BUILD__/$BUILD/" packaging/Info.plist > "$APP/Contents/Info.plist"
if [ -n "${BUNDLE_ID:-}" ]; then
  /usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $BUNDLE_ID" "$APP/Contents/Info.plist"
fi
cp packaging/Dostoevsky.icns "$APP/Contents/Resources/"
# One .lproj per UI language (CFBundleLocalizations), so AppKit's own text follows the app's.
for code in en $(cd crates/l10n/locales && ls *.json | sed 's/\.json$//'); do
  mkdir -p "$APP/Contents/Resources/$code.lproj"
  : > "$APP/Contents/Resources/$code.lproj/InfoPlist.strings"
done
cp crates/app/assets/fonts/OFL-*.txt LICENSE NOTICE packaging/PrivacyInfo.xcprivacy "$APP/Contents/Resources/"
# Apache/MIT/BSD ask for their notices to travel with the binary (dsky-a8n.3).
if command -v cargo-about >/dev/null; then
  cargo about generate --frozen packaging/about.hbs -o "$APP/Contents/Resources/THIRD-PARTY-LICENSES.txt"
elif [ -n "${SIGN_IDENTITY:-}" ]; then
  echo "cargo-about is needed for a release build (brew install cargo-about)" >&2; exit 1
fi

IDENTITY="${SIGN_IDENTITY:--}"

# Notarization wants a secure timestamp; an ad-hoc build skips the round trip to Apple's server.
if [ "$IDENTITY" = "-" ]; then
  codesign --force --options runtime --timestamp=none --entitlements packaging/entitlements.plist \
    --sign "$IDENTITY" "$APP" 2>/dev/null \
    || codesign --force --options runtime --entitlements packaging/entitlements.plist --sign "$IDENTITY" "$APP"
else
  codesign --force --options runtime --timestamp --entitlements packaging/entitlements.plist --sign "$IDENTITY" "$APP"
fi
codesign --verify --strict --verbose=2 "$APP"
if [ -n "${NO_DMG:-}" ]; then
  echo "Built $APP ($(lipo -archs "$APP/Contents/MacOS/dostoevsky")); NO_DMG, so no .dmg"
  exit 0
fi

# The disk image holds the app beside an Applications link, so it installs by dragging one
# onto the other. Built read-write first so Finder can lay out the window, then compressed.
DMG="$OUT/Dostoevsky-$VERSION.dmg"
STAGE="$OUT/dmg"
RW="$OUT/Dostoevsky-rw.dmg"
rm -rf "$STAGE" "$DMG" "$RW"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -quiet -volname "Dostoevsky" -srcfolder "$STAGE" -ov -format UDRW "$RW"
MOUNT="$(hdiutil attach -readwrite -noverify -noautoopen "$RW" | awk -F'\t' '/\/Volumes\//{print $NF}')"
# An already-mounted "Dostoevsky" volume makes this one "Dostoevsky 1"; address it by its own name.
VOL="$(basename "$MOUNT")"
for _ in 1 2 3 4 5 6 7 8 9 10; do   # Finder sees a fresh mount a moment later
  osascript -e "tell application \"Finder\" to exists disk \"$VOL\"" 2>/dev/null | grep -q true && break
  sleep 0.5
done
# Window layout is cosmetic: without Finder (CI, no Automation permission) the icons just
# take default positions.
osascript >/dev/null 2>&1 <<APPLESCRIPT || echo "note: Finder layout skipped; the .dmg still has the Applications link"
tell application "Finder"
  tell disk "$VOL"
    open
    set current view of container window to icon view
    set toolbar visible of container window to false
    set statusbar visible of container window to false
    set the bounds of container window to {200, 120, 740, 460}
    set opts to the icon view options of container window
    set arrangement of opts to not arranged
    set icon size of opts to 128
    set position of item "Dostoevsky.app" of container window to {140, 150}
    set position of item "Applications" of container window to {400, 150}
    update without registering applications
    delay 1
    close
  end tell
end tell
APPLESCRIPT
sync
for _ in 1 2 3 4 5; do hdiutil detach -quiet "$MOUNT" && break; sleep 1; done
[ -d "$MOUNT" ] && hdiutil detach -quiet -force "$MOUNT"
hdiutil convert -quiet "$RW" -format UDZO -o "$DMG"
rm -rf "$STAGE" "$RW"
[ "$IDENTITY" != "-" ] && codesign --force --sign "$IDENTITY" "$DMG"

notary=()
if [ -n "${NOTARY_PROFILE:-}" ]; then
  notary=(--keychain-profile "$NOTARY_PROFILE")
elif [ -n "${NOTARY_KEY_ID:-}" ]; then
  [ -n "${NOTARY_ISSUER:-}" ] || { echo "NOTARY_KEY_ID needs NOTARY_ISSUER" >&2; exit 1; }
  notary=(--key "target/secrets/AuthKey_$NOTARY_KEY_ID.p8"
          --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER")
fi
if [ "${#notary[@]}" -gt 0 ]; then
  xcrun notarytool submit "$DMG" "${notary[@]}" --wait
  xcrun stapler staple "$APP"
  xcrun stapler staple "$DMG"
  spctl --assess --type open --context context:primary-signature -v "$DMG"
fi

echo "Built $APP and $DMG ($(lipo -archs "$APP/Contents/MacOS/dostoevsky"))"
