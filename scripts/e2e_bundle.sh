#!/usr/bin/env bash
# End-to-end tests on the bundled app, launched the way Finder launches it.
#
#   scripts/e2e_bundle.sh                  # build and exercise the ordinary .dmg app
#   NO_BUILD=1 scripts/e2e_bundle.sh       # reuse target/e2e/dmg/Dostoevsky.app
#
# What the headless tests (cargo test) and the flows (run_flows.sh) can't reach: the real
# Info.plist, entitlements and signature; files handed over by LaunchServices
# (`open`, like Finder's Open With and Dock drops) rather than argv; real FSEvents; a real
# kill -9 and relaunch. The build is the shipped packaging from scripts/bundle.sh with the
# scripted driver compiled in (`--features shots`, DOSTOEVSKY_SCRIPT) and its own bundle
# identifier, so a run never touches an installed Dostoevsky or its recent files.
# Needs a logged-in macOS session (a window server); runs on CI's macOS runners.
# Builds and artifacts stay below target/; the run's own scratch goes to the system temp
# dir (E2E_SCRATCH overrides — CI points it back under target/).
#
# A failure listed in KNOWN is reported and doesn't fail the run. A known failure that starts
# passing fails the run until it's taken off the list.
set -euo pipefail
cd "$(dirname "$0")/.."

ARCH="$(uname -m)"; [ "$ARCH" = arm64 ] && ARCH=aarch64
ROOT="$PWD/target/e2e"
RUN="$(date +%Y%m%d-%H%M%S)-$$"
# Documents, logs and support data go to the system temp dir: a fresh app may not write into
# ~/Documents or ~/Desktop (macOS privacy) — and a checkout often sits inside ~/Documents —
# so a --stdout path there fails the launch itself (LaunchServices error -10810). CI sets
# E2E_SCRATCH=target/e2e/scratch instead: runners have no such privacy wall, and the logs
# and screenshots then ride along with the target/e2e artifacts.
SCRATCH="${E2E_SCRATCH:-${TMPDIR:-/tmp}/dostoevsky-e2e}"
# Absolute: the app starts in / under LaunchServices, and a relative DOSTOEVSKY_SUPPORT_DIR
# there can't be made, so recent files and crash journals went nowhere.
case "$SCRATCH" in /*) ;; *) SCRATCH="$PWD/$SCRATCH" ;; esac

# variant/test → the bead that tracks it. Only a test's main result (the app run) counts;
# its file checks fail along with it.
KNOWN=(
)

fail=0
known_for() { local k; for k in "${KNOWN[@]+"${KNOWN[@]}"}"; do [ "${k%% *}" = "$1" ] && { echo "${k#* }"; return 0; }; done; return 0; }

# result <variant> <test> <ok 0|1> <what> [log] — the test's main result (KNOWN applies).
result() {
  local v=$1 t=$2 ok=$3 what=$4 log=${5:-} bead
  bead=$(known_for "$v/$t")
  if [ "$ok" = 0 ] && [ -n "$bead" ]; then
    echo "FAIL  $v/$t: $what — passes now, take it off KNOWN ($bead)"; fail=1
  elif [ "$ok" = 0 ]; then
    echo "ok    $v/$t: $what"
  elif [ -n "$bead" ]; then
    echo "known $v/$t: $what ($bead)"
  else
    echo "FAIL  $v/$t: $what"; fail=1
    if [ -n "$log" ] && [ -f "$log" ]; then grep -v '^[[:space:]]*$' "$log" | tail -8 | sed 's/^/        /' || true; fi
  fi
}
check() { # variant test what command…: a check on what a test left behind
  local v=$1 t=$2 what=$3 bead; shift 3
  bead=$(known_for "$v/$t")
  if "$@" >/dev/null 2>&1; then echo "ok    $v/$t: $what"
  elif [ -n "$bead" ]; then echo "known $v/$t: $what ($bead)"
  else echo "FAIL  $v/$t: $what"; fail=1; fi
}
main() { # variant test what command…: a test's main result
  local v=$1 t=$2 what=$3; shift 3
  if "$@" >/dev/null 2>&1; then result "$v" "$t" 0 "$what"; else result "$v" "$t" 1 "$what"; fi
}

# The app's pid, once it's up (the driver is ours alone: target/e2e/<variant>/…).
app_pid() { pgrep -f "$APP/Contents/MacOS/dostoevsky" | head -1; }

# launch <name> <script> [path…]: starts the app through LaunchServices in the background,
# hidden and without focus, its stderr in $WORK/<name>.err. Sets OPEN_PID.
launch() {
  local name=$1 script=$2; shift 2
  : >"$WORK/$name.err"
  open -n -g -W -a "$APP" --stdout "$WORK/$name.out" --stderr "$WORK/$name.err" \
    --env "DOSTOEVSKY_SCRIPT=$script" --env "DOSTOEVSKY_SUPPORT_DIR=$SUPPORT" \
    --env "DOSTOEVSKY_KEYMAP=$WORK/no-keymap.json" "$@" &
  OPEN_PID=$!
}
# finish <seconds>: waits for the launched app to quit; kills it if it hangs. Status 0 when
# the script ran to the end with every expectation met.
finish() {
  local n=$(( $1 * 10 )) name=$2
  while kill -0 "$OPEN_PID" 2>/dev/null && [ $n -gt 0 ]; do sleep 0.1; n=$((n - 1)); done
  if kill -0 "$OPEN_PID" 2>/dev/null; then
    echo "shots: e2e: still running after $1 s, killed" >>"$WORK/$name.err"
    kill -9 "$(app_pid)" 2>/dev/null || true
    wait "$OPEN_PID" 2>/dev/null || true
    return 1
  fi
  wait "$OPEN_PID" 2>/dev/null || true
  ! grep -q 'shots: expect failed' "$WORK/$name.err" && grep -q '^shots: done' "$WORK/$name.err"
}
# said <name> <word> <seconds>: waits for the script's {"say": word}.
said() {
  local n=$(( $3 * 10 ))
  until grep -q "^shots: say $2\$" "$WORK/$1.err" 2>/dev/null; do
    [ $n -gt 0 ] || return 1; sleep 0.1; n=$((n - 1))
  done
}
# One JSON step per argument → the inline script, in hex (`open --env` drops values with spaces).
steps() { printf 'hex:'; printf '%s\n' "$@" | xxd -p | tr -d '\n'; }

# A name with a space and Cyrillic: Finder sends it percent-encoded (main.rs file_paths).
NAME='Мой черновик.md'
DOC='Most of the friction in writing comes from choosing too early.

Editors have always known this.
'

for v in dmg; do
  OUT="target/e2e/$v"; APP="$PWD/$OUT/Dostoevsky.app"; ID="app.dostoevsky.Dostoevsky.e2e-$v"
  if [ -z "${NO_BUILD:-}" ]; then
    echo "== $v: building $APP"
    ARCHS=$ARCH FEATURES=shots BUNDLE_ID=$ID OUT=$OUT NO_DMG=1 scripts/bundle.sh >"$ROOT-$v-build.log" 2>&1 \
      || { echo "FAIL  $v: build (see $ROOT-$v-build.log)"; tail -20 "$ROOT-$v-build.log"; fail=1; continue; }
  fi
  echo "== $v: $(lipo -archs "$APP/Contents/MacOS/dostoevsky"), $ID"
  WORK="$SCRATCH/$v-$RUN"; mkdir -p "$WORK"
  support() {
    SUPPORT="$WORK/support-$1"; mkdir -p "$SUPPORT"
  }

  # 1. Open from Finder, edit, save (FILE-1). The folder holds nothing but the file after.
  t=save; support $t; mkdir -p "$WORK/$t"; printf '%s' "$DOC" >"$WORK/$t/$NAME"
  launch $t "$(steps \
    '{"wait_for": {"text_contains": "Editors have always known this."}, "timeout": 10000}' \
    '{"expect": {"path_ends": "'"$NAME"'", "dirty": false}}' \
    '{"select": {"text": "Most", "at": "start"}}' \
    '{"type": "E2E. "}' \
    '{"keys": "cmd-s"}' \
    '{"wait_for": {"dirty": false}, "timeout": 3000}')" "$WORK/$t/$NAME"
  if finish 30 $t; then result $v $t 0 "opened from Finder, edited, saved"; else result $v $t 1 "opened from Finder, edited, saved" "$WORK/$t.err"; fi
  check $v $t "the edit is in the file" grep -q '^E2E. Most of the friction' "$WORK/$t/$NAME"
  check $v $t "no stray files beside it" bash -c "[ \"\$(ls -A '$WORK/$t')\" = '$NAME' ]"

  # 2. Relaunch with nothing to open: the last document comes back (FILE-1, recent files).
  t=recent; support $t; mkdir -p "$WORK/$t"; printf '%s' "$DOC" >"$WORK/$t/$NAME"
  launch $t-1 "$(steps '{"wait_for": {"path_ends": "'"$NAME"'"}, "timeout": 10000}')" "$WORK/$t/$NAME"
  finish 30 $t-1 || true
  launch $t "$(steps '{"wait_for": {"path_ends": "'"$NAME"'", "text_contains": "Editors have always"}, "timeout": 5000}')"
  if finish 30 $t; then result $v $t 0 "relaunch reopens the last document"; else result $v $t 1 "relaunch reopens the last document" "$WORK/$t.err"; fi

  # 3. A folder from Finder or the Dock opens as a book (PROJ-1), and a chapter saves.
  main $v plist "Info.plist takes folders (Dock drops, PROJ-1)" \
    bash -c "plutil -extract CFBundleDocumentTypes json -o - '$APP/Contents/Info.plist' | grep -q public.folder"
  t=folder; support $t; cp -R scripts/flows/book "$WORK/$t"
  launch $t "$(steps \
    '{"wait": 500}' \
    '{"first_open": "confirm"}' \
    '{"wait_for": {"project": true, "text_contains": "On an exceptionally hot evening"}, "timeout": 10000}' \
    '{"select": {"text": "On an exceptionally", "at": "start"}}' \
    '{"type": "E2E. "}' \
    '{"keys": "cmd-s"}' \
    '{"wait_for": {"dirty": false}, "timeout": 3000}')" "$WORK/$t"
  if finish 30 $t; then result $v $t 0 "folder opens as a book, a chapter saves"; else result $v $t 1 "folder opens as a book, a chapter saves" "$WORK/$t.err"; fi
  check $v $t "the edit is in the chapter" grep -q 'E2E. On an exceptionally' "$WORK/$t/part-1/01-the-garret.md"
  check $v $t "the type is kept in the folder" grep -q 'kind = "book"' "$WORK/$t/.dostoevsky"

  # 3b. Relaunch with nothing to open: the book comes back, and a chapter still saves.
  t=reopen-folder; support $t; cp -R scripts/flows/book "$WORK/$t"
  launch $t-1 "$(steps \
    '{"wait": 500}' \
    '{"first_open": "confirm"}' \
    '{"wait_for": {"project": true, "text_contains": "On an exceptionally hot evening"}, "timeout": 10000}')" "$WORK/$t"
  finish 30 $t-1 || true
  launch $t "$(steps \
    '{"wait_for": {"project": true, "text_contains": "On an exceptionally hot evening"}, "timeout": 5000}' \
    '{"select": {"text": "On an exceptionally", "at": "start"}}' \
    '{"type": "Again. "}' \
    '{"keys": "cmd-s"}' \
    '{"wait_for": {"dirty": false}, "timeout": 3000}')"
  if finish 30 $t; then result $v $t 0 "relaunch reopens the book, a chapter saves"; else result $v $t 1 "relaunch reopens the book, a chapter saves" "$WORK/$t.err"; fi
  check $v $t "the edit is in the chapter" grep -q 'Again. On an exceptionally' "$WORK/$t/part-1/01-the-garret.md"

  # 4. kill -9 with unsaved work, relaunch: the work comes back (FILE-4). The journal is
  #    written every 5 s; autosave is off so the file itself stays as it was.
  t=crash; support $t; mkdir -p "$WORK/$t"; printf '%s' "$DOC" >"$WORK/$t/$NAME"
  launch $t-1 "$(steps \
    '{"prefs": {"autosave": "off"}}' \
    '{"wait_for": {"text_contains": "Editors have always"}, "timeout": 10000}' \
    '{"select": {"text": "Most", "at": "start"}}' \
    '{"type": "Unsaved before the crash. "}' \
    '{"say": "typed"}' \
    '{"wait": 60000}')" "$WORK/$t/$NAME"
  if said $t-1 typed 15; then
    sleep 7
    kill -9 "$(app_pid)" 2>/dev/null || true
  fi
  finish 10 $t-1 || true
  check $v $t "the file on disk is untouched" bash -c "! grep -q 'Unsaved before' '$WORK/$t/$NAME'"
  # The recovered page still belongs to its file, which saves.
  launch $t "$(steps \
    '{"wait_for": {"text_contains": "Unsaved before the crash.", "dirty": true}, "timeout": 5000}' \
    '{"keys": "cmd-s"}' \
    '{"wait_for": {"dirty": false}, "timeout": 3000}')"
  if finish 30 $t; then result $v $t 0 "unsaved work survives kill -9 and saves"; else result $v $t 1 "unsaved work survives kill -9 and saves" "$WORK/$t.err"; fi
  check $v $t "the recovered work is in the file" grep -q '^Unsaved before the crash. Most' "$WORK/$t/$NAME"

  # 5. Another editor changes the file: the page follows (FILE-5, real FSEvents).
  t=outside; support $t; mkdir -p "$WORK/$t"; printf '%s' "$DOC" >"$WORK/$t/$NAME"
  launch $t "$(steps \
    '{"wait_for": {"text_contains": "Editors have always"}, "timeout": 10000}' \
    '{"wait": 500}' \
    '{"say": "ready"}' \
    '{"wait_for": {"text_contains": "Changed in another editor.", "dirty": false}, "timeout": 10000}')" "$WORK/$t/$NAME"
  said $t ready 15 && printf 'Changed in another editor.\n' >"$WORK/$t/$NAME"
  if finish 30 $t; then result $v $t 0 "an outside change reloads the page"; else result $v $t 1 "an outside change reloads the page" "$WORK/$t.err"; fi
done

if [ $fail = 0 ]; then rm -rf "$SCRATCH"/*-"$RUN"; else echo "logs: $SCRATCH/*-$RUN"; fi
exit $fail
