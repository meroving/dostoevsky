#!/usr/bin/env bash
# Dostoevsky for Linux as an AppImage (dsky-fbso.13): one file that runs on x86_64 desktops with
# the glibc of the system it was built on or newer, with its .desktop entry, icons and AppStream
# metadata inside. CI builds it on Ubuntu 22.04 (.github/workflows/ci.yml, release.yml).
#
#   scripts/appimage.sh              # target/bundle/Dostoevsky-<version>-x86_64.AppImage
#   UPDATES=1 scripts/appimage.sh    # with the update check (updates.rs): the project's releases only
#
# Needs GPUI's build libraries (ci.yml lists them), curl, and cargo-about for the third-party
# licenses (cargo install cargo-about). No FUSE needed: appimagetool runs extracted.
#
# Libraries: the AppImage carries none. Everything the binary links is on every desktop that can
# draw a window (Wayland or X11, fontconfig, FreeType, ALSA, zstd); Vulkan is opened at runtime.
# A library outside that list fails the build, so a new one is a decision, not an accident.
set -euo pipefail
cd "$(dirname "$0")/.."

ID=app.dostoevsky.Dostoevsky
ARCH="$(uname -m)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
OUT=target/bundle
WORK=target/appimage
APPDIR="$WORK/AppDir"
APPIMAGE="$OUT/Dostoevsky-$VERSION-$ARCH.AppImage"

# appimagetool and the runtime it puts in front of the image, pinned by version and checksum.
APPIMAGETOOL_URL=https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
APPIMAGETOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
RUNTIME_URL=https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64
RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d

[ "$ARCH" = x86_64 ] || { echo "only x86_64 so far (the tools are pinned for it)" >&2; exit 1; }

FEATURES=""
[ "${UPDATES:-}" = 1 ] && FEATURES=updates
cargo build --release --locked -p dostoevsky ${FEATURES:+--features "$FEATURES"}
BIN=target/release/dostoevsky

# What the binary links, against what every desktop has.
ALLOWED='^(ld-linux-x86-64\.so\.2|libc\.so\.6|libm\.so\.6|libdl\.so\.2|libpthread\.so\.0|librt\.so\.1|libgcc_s\.so\.1|libxkbcommon(-x11)?\.so\.0|libwayland-(client|cursor|egl)\.so\.0|libX11(-xcb)?\.so\.6|libX11-xcb\.so\.1|libxcb[a-z0-9-]*\.so\.[0-9]+|libfontconfig\.so\.1|libfreetype\.so\.6|libasound\.so\.2|libzstd\.so\.1|libz\.so\.1)$'
needed="$(readelf -d "$BIN" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p')"
echo "links: $(echo $needed)"
unexpected="$(echo "$needed" | grep -Ev "$ALLOWED" || true)"
if [ -n "$unexpected" ]; then
  echo "the binary links libraries a desktop may not have: $(echo $unexpected)" >&2
  echo "bundle them into the AppImage or add them to ALLOWED in $0" >&2
  exit 1
fi
echo "needs glibc $(objdump -T "$BIN" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -Vu | tail -1) or newer"

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/metainfo" \
  "$APPDIR/usr/share/doc/dostoevsky" "$OUT"
install -m 755 "$BIN" "$APPDIR/usr/bin/dostoevsky"
strip "$APPDIR/usr/bin/dostoevsky"
cp "packaging/linux/$ID.desktop" "$APPDIR/usr/share/applications/"
sed -e "s/__VERSION__/$VERSION/" -e "s/__DATE__/$(date -u +%Y-%m-%d)/" \
  "packaging/linux/$ID.metainfo.xml" > "$APPDIR/usr/share/metainfo/$ID.metainfo.xml"
for px in 256 512; do
  mkdir -p "$APPDIR/usr/share/icons/hicolor/${px}x${px}/apps"
  cp "packaging/linux/icon-$px.png" "$APPDIR/usr/share/icons/hicolor/${px}x${px}/apps/$ID.png"
done
cp LICENSE NOTICE crates/app/assets/fonts/OFL-*.txt "$APPDIR/usr/share/doc/dostoevsky/"
# Apache/MIT/BSD ask for their notices to travel with the binary (dsky-a8n.3).
if command -v cargo-about >/dev/null; then
  # The license list covers every system's crates (about.toml), which a build for one doesn't download.
  cargo fetch --locked --quiet
  cargo about generate --frozen packaging/about.hbs -o "$APPDIR/usr/share/doc/dostoevsky/THIRD-PARTY-LICENSES.txt"
else
  echo "cargo-about is needed for a release build (cargo install cargo-about)" >&2; exit 1
fi

# The AppDir's own entry points: the .desktop file and icon at its root, and AppRun.
ln -s "usr/share/applications/$ID.desktop" "$APPDIR/$ID.desktop"
ln -s "usr/share/icons/hicolor/256x256/apps/$ID.png" "$APPDIR/$ID.png"
ln -s "$ID.png" "$APPDIR/.DirIcon"
cat > "$APPDIR/AppRun" <<'APPRUN'
#!/bin/sh
here="$(dirname "$(readlink -f "$0")")"
exec "$here/usr/bin/dostoevsky" "$@"
APPRUN
chmod 755 "$APPDIR/AppRun"

fetch() { # url sha256 path
  if [ ! -f "$3" ] || ! echo "$2  $3" | sha256sum -c --quiet - 2>/dev/null; then
    curl -sSLf --retry 3 -o "$3.part" "$1"
    echo "$2  $3.part" | sha256sum -c --quiet -
    mv "$3.part" "$3"
  fi
}
mkdir -p "$WORK/tools"
fetch "$APPIMAGETOOL_URL" "$APPIMAGETOOL_SHA256" "$WORK/tools/appimagetool"
fetch "$RUNTIME_URL" "$RUNTIME_SHA256" "$WORK/tools/runtime"
chmod 755 "$WORK/tools/appimagetool"

rm -f "$APPIMAGE"
APPIMAGE_EXTRACT_AND_RUN=1 ARCH="$ARCH" VERSION="$VERSION" \
  "$WORK/tools/appimagetool" --no-appstream --runtime-file "$WORK/tools/runtime" "$APPDIR" "$APPIMAGE"
echo "$APPIMAGE"
