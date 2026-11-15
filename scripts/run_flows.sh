#!/usr/bin/env bash
# Runs the PRD §7 key flows against the real app (scripted, rendered offscreen) and checks
# what was saved. Screenshots land in target/flows/. Needs macOS with a GPU.
#
#   scripts/run_flows.sh [flow-name-prefix…]
#   UPDATE_BASELINES=1 scripts/run_flows.sh   # approve the screenshots as the new baselines
#   BASELINES=target/tmp scripts/run_flows.sh # compare against another baseline folder
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -q -p dostoevsky --features shots
BIN=target/debug/dostoevsky
OUT="$PWD/target/flows"
rm -rf "$OUT" && mkdir -p "$OUT/support"
export DOSTOEVSKY_SUPPORT_DIR="$OUT/support"
# The baselines are English, whatever languages this Mac has (DOSTOEVSKY_LANG=ru to look).
export DOSTOEVSKY_LANG="${DOSTOEVSKY_LANG:-en}"
# The baselines are dark; a fresh install would match whatever this Mac is set to.
echo '{"appearance": "dark", "zen_writing": false, "typewriter": false, "panels": {"editing": {"left": null}}}' > "$OUT/support/prefs.json"
fail=0

run() { # name fixture
  local name=$1 fixture=$2 doc="$OUT/$1.md"
  cp "scripts/flows/$fixture" "$doc"
  sed "s#\$OUT#$OUT#g" "scripts/flows/$name.jsonl" > "$OUT/$name.jsonl"
  local status=0
  DOSTOEVSKY_SCRIPT="$OUT/$name.jsonl" timeout 60 "$BIN" "$doc" >"$OUT/$name.log" 2>&1 || status=$?
  echo "$status" >"$OUT/$name.status"
  echo "$doc"
}
expects() { # name: the script's own {"expect": …} steps passed (exit status 0)
  local name=$1 short=${1%%-*}
  if [ "$(cat "$OUT/$name.status")" = 0 ]; then
    echo "ok    $short: script expectations"
  else
    echo "FAIL  $short: script expectations (exit $(cat "$OUT/$name.status"))"
    grep 'shots: expect failed' "$OUT/$name.log" | sed 's/^/        /'
    fail=1
  fi
}
check() { # name description command…
  local name=$1 what=$2; shift 2
  if "$@" >/dev/null 2>&1; then echo "ok    $name: $what"; else echo "FAIL  $name: $what"; fail=1; fi
}
want() { [ $# -eq 0 ] || [[ " $* " == *" $1 "* ]]; }
sel=("$@")
pick() { [ ${#sel[@]} -eq 0 ] && return 0; for s in "${sel[@]}"; do [[ $1 == $s* ]] && return 0; done; return 1; }

if pick flow1; then d=$(run flow1-try-another-word keep-the-words.md); expects flow1-try-another-word
  check flow1 "group saved, back on the original after a full cycle" grep -q '"active":0,"variants":\["friction","hesitation","struggle","tension"\]' "$d"
  check flow1 "body shows the original" grep -q 'Most of the friction' "$d"; fi
if pick flow2; then d=$(run flow2-add-your-own keep-the-words.md); expects flow2-add-your-own
  check flow2 "the new variant shows: 'write a comment'" grep -q 'They write a comment in the margin' "$d"
  check flow2 "three variants, mine" grep -q '"variants":\["suggestion","note","comment"\]' "$d"; fi
if pick flow3; then d=$(run flow3-paragraph-rewrite keep-the-words.md); expects flow3-paragraph-rewrite
  check flow3 "paragraph group saved" grep -q '"scope":"paragraph"' "$d"; fi
if pick flow4; then d=$(run flow4-ghost-instead-of-delete keep-the-words.md); expects flow4-ghost-instead-of-delete
  check flow4 "the cut is saved with its trace" grep -q '"trace":true' "$d"; fi
if pick flow5; then d=$(run flow5-stash-and-bring-back keep-the-words.md); expects flow5-stash-and-bring-back
  check flow5 "the paragraph came back above the last one" bash -c "grep -B2 '^Software forgot this' '$d' | grep -q '^Editors have always known this'"
  check flow5 "no overflow left" bash -c "! grep -q 'dostoevsky:overflow' '$d'"; fi
if pick flow6; then d=$(run flow6-finish keep-the-words.md); expects flow6-finish
  check flow6 "settled: plain Markdown, no trailer" bash -c "! grep -q 'dostoevsky:meta' '$d'"
  check flow6 "settled on the chosen word" grep -q 'Most of the tension' "$d"; fi

if pick flow8; then
  # A project: the script gets a copy of a folder, not a file.
  book="$OUT/crime-and-punishment"; cp -R scripts/flows/book "$book"
  sed "s#\$OUT#$OUT#g" scripts/flows/flow8-write-a-book.jsonl > "$OUT/flow8-write-a-book.jsonl"
  status=0; DOSTOEVSKY_SCRIPT="$OUT/flow8-write-a-book.jsonl" timeout 60 "$BIN" "$book" >"$OUT/flow8-write-a-book.log" 2>&1 || status=$?
  echo "$status" >"$OUT/flow8-write-a-book.status"
  expects flow8-write-a-book
  check flow8 "the new chapter landed after the current one" test -f "$book/part-1/03-the-visit.md"
  check flow8 "later chapters moved down" test -f "$book/part-1/04-the-letter.md"
  check flow8 "what was written is in the file" grep -q 'A knock at the door.' "$book/part-1/03-the-visit.md"
  check flow8 "the type is saved in the folder" grep -q 'kind = "book"' "$book/.dostoevsky"
  check flow8 "nothing else in the folder" bash -c "[ \$(find '$book' -type f | wc -l) -eq 7 ]"; fi

if pick flow9; then run flow9-start-a-project keep-the-words.md >/dev/null; expects flow9-start-a-project
  check flow9 "the first chapter was made" test -f "$OUT/the-idiot/01-untitled.md"
  check flow9 "and the notes folder" test -d "$OUT/the-idiot/notes"
  check flow9 "the type is saved in the folder" grep -q 'kind = "book"' "$OUT/the-idiot/.dostoevsky"; fi

# Visual regression: compare each screenshot with the approved baseline made on the same
# kind of machine (macOS version and screen scale change the rendering). Make or refresh
# baselines with UPDATE_BASELINES=1; a machine without any is skipped with a note.
shopt -s nullglob
shots=("$OUT"/*.png)
if [ ${#shots[@]} -gt 0 ]; then
  width=$(sips -g pixelWidth "${shots[0]}" | awk '/pixelWidth/ {print $2}')
  KEY="macos$(sw_vers -productVersion | cut -d. -f1)-${width}px"
  BASE="${BASELINES:-scripts/flows/baselines}/$KEY"
  if [ "${UPDATE_BASELINES:-0}" = 1 ] && [ $fail != 0 ]; then
    echo "FAIL  visual: a flow failed above; not approving these screenshots"
  elif [ "${UPDATE_BASELINES:-0}" = 1 ]; then
    mkdir -p "$BASE"
    for s in "${shots[@]}"; do
      # Recompressed losslessly (about half the size) when ImageMagick is around.
      magick "$s" -define png:compression-level=9 -define png:compression-filter=5 -strip "$BASE/$(basename "$s")" 2>/dev/null || cp "$s" "$BASE/"
    done
    echo "baselines: updated ${#shots[@]} in $BASE"
  elif [ ! -d "$BASE" ]; then
    echo "note  visual: no baselines for $KEY (UPDATE_BASELINES=1 scripts/run_flows.sh to approve these)"
  else
    for s in "${shots[@]}"; do
      name=$(basename "$s" .png)
      if [ ! -f "$BASE/$name.png" ]; then
        echo "note  visual: $name has no baseline"
        continue
      fi
      if r=$(target/debug/imgdiff "$BASE/$name.png" "$s" "$OUT/$name.diff.png"); then
        echo "ok    visual: $name ($r)"
      else
        echo "FAIL  visual: $name ($r) → $OUT/$name.diff.png"
        fail=1
      fi
    done
  fi
fi

echo "screenshots: $OUT"
exit $fail
