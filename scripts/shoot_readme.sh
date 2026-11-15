#!/usr/bin/env bash
# Renders the README's screenshots from the real app into docs/screens/.
#
#   scripts/shoot_readme.sh [shot…]       Needs macOS with a GPU (and ImageMagick to shrink them).
#   APPEARANCE=light DEST=target/readme-light scripts/shoot_readme.sh [shot…]   the same shots on paper
#
# Each scripts/readme/<shot>.jsonl runs against a fresh copy of scripts/readme/sample.md
# (or the book, for shots named project-*; or scripts/readme/book with the sample as its first
# chapter, for the site's mode-* frames). AI shots talk to scripts/readme/stub_ai.py,
# a stand-in model on 127.0.0.1 that answers with canned replies: no key, no network.
set -euo pipefail
# English, whatever languages this Mac has (DOSTOEVSKY_LANG=ru for Russian shots).
export DOSTOEVSKY_LANG="${DOSTOEVSKY_LANG:-en}"
cd "$(dirname "$0")/.."
cargo build -q -p dostoevsky --features shots
BIN=target/debug/dostoevsky
OUT="$PWD/target/readme"
DEST="${DEST:-docs/screens}"
case "$DEST" in
  /*|..|../*|*/..|*/../*) echo "DEST must stay inside the repository" >&2; exit 1 ;;
esac
DEST="$PWD/$DEST"
APPEARANCE="${APPEARANCE:-}"   # empty: each shot's own
mkdir -p "$OUT" "$DEST"
DEST="$(cd "$DEST" && pwd -P)"
case "$DEST" in
  "$PWD"/*) ;;
  *) echo "DEST must stay inside the repository" >&2; exit 1 ;;
esac
shots=("$@")
[ ${#shots[@]} -gt 0 ] || shots=($(cd scripts/readme && ls *.jsonl | sed 's/\.jsonl$//'))

STUB_PORT="${STUB_PORT:-11434}"   # another port when a real Ollama holds 11434
stub=""
cleanup() { [ -z "$stub" ] || kill "$stub" 2>/dev/null || true; }
trap cleanup EXIT

for s in "${shots[@]}"; do
  script="scripts/readme/$s.jsonl"
  support="$OUT/support-$s"
  rm -rf "$support" && mkdir -p "$support"
  if [[ $s == ai-* ]]; then
    if [ -z "$stub" ]; then
      python3 -I scripts/readme/stub_ai.py "$STUB_PORT" >"$OUT/stub_ai.log" 2>&1 &
      stub=$!
      sleep 0.5
    fi
    cat > "$support/prefs.json" <<JSON
{"appearance": "dark", "zen_writing": false, "typewriter": false, "ai": {"services": [{"kind": "open-router"}, {"kind": "ollama", "base_url": "http://127.0.0.1:$STUB_PORT/v1"}],
 "quick": {"service": "ollama", "model": "llama3.2:3b"}, "deep": {"service": "ollama", "model": "qwen3:32b"}}}
JSON
  elif [ -f "scripts/readme/$s.prefs.json" ]; then
    cp "scripts/readme/$s.prefs.json" "$support/prefs.json"   # a shot that needs its own settings
  else
    echo '{"appearance": "dark", "zen_writing": false, "typewriter": false, "panels": {"editing": {"left": null}}}' > "$support/prefs.json"
  fi
  [ -z "$APPEARANCE" ] || sed -i '' -E "s/\"appearance\": \"[a-z]+\"/\"appearance\": \"$APPEARANCE\"/" "$support/prefs.json"
  sed "s#\$OUT#$OUT#g" "$script" > "$OUT/$s.jsonl"
  work="$OUT/work-$s"; rm -rf "$work" && mkdir -p "$work"
  if [[ $s == project-* ]]; then
    doc="$work/crime-and-punishment"; cp -R scripts/flows/book "$doc"
  elif [[ $s == mode-* ]]; then
    # The site's writing/editing frames: the sample as chapter 1 of a small book.
    doc="$work/on-writing"; cp -R scripts/readme/book "$doc"
    cp scripts/readme/sample.md "$doc/01-keep-the-words.md"
  else
    sample=scripts/readme/sample.md
    [[ $s == ai-* ]] && sample=scripts/readme/sample-ai.md
    doc="$work/keep-the-words.md"; cp "$sample" "$doc"
  fi
  DOSTOEVSKY_SECRETS=memory DOSTOEVSKY_WINDOW_SIZE=1440x900 DOSTOEVSKY_SUPPORT_DIR="$support" \
    DOSTOEVSKY_SCRIPT="$OUT/$s.jsonl" timeout 60 "$BIN" "$doc" >"$OUT/$s.log" 2>&1 || echo "FAIL  $s (see $OUT/$s.log)"
  for png in "$OUT/$s"*.png; do
    [ -f "$png" ] || continue
    # Half size (1440 px wide) keeps the repo light and is still sharp on GitHub.
    magick "$png" -resize 50% -strip -define png:compression-level=9 "$DEST/$(basename "$png")"
    rm "$png"
  done
  echo "$s"
done
echo "screenshots: $DEST"
