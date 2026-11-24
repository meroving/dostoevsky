#!/usr/bin/env bash
# Records a demo video from the real app: scripts/demo/<name>.jsonl (with <name>.prefs.json)
# against a fresh copy of scripts/readme/sample.md, through the shots runner's `record` step.
#
#   scripts/demo_video.sh [name]     → target/demo/<name>.mp4 (1920×1200, H.264; default: launch)
#
# Needs macOS with a GPU and ffmpeg. SPEED (default 1.1) speeds the take up in the final encode.
set -euo pipefail
export DOSTOEVSKY_LANG="${DOSTOEVSKY_LANG:-en}"
cd "$(dirname "$0")/.."
NAME="${1:-launch}"
SPEED="${SPEED:-1.1}"
cargo build -q --release -p dostoevsky --features shots
OUT="$PWD/target/demo/$NAME"
rm -rf "$OUT" && mkdir -p "$OUT/support"
cp "scripts/demo/$NAME.prefs.json" "$OUT/support/prefs.json"
sed "s#\$OUT#$OUT#g" "scripts/demo/$NAME.jsonl" > "$OUT/steps.jsonl"
cp scripts/readme/sample.md "$OUT/keep-the-words.md"
DOSTOEVSKY_SECRETS=memory DOSTOEVSKY_WINDOW_SIZE=1088x680 DOSTOEVSKY_SUPPORT_DIR="$OUT/support" \
  DOSTOEVSKY_SCRIPT="$OUT/steps.jsonl" timeout 120 target/release/dostoevsky "$OUT/keep-the-words.md" >"$OUT/log" 2>&1 \
  || { echo "FAIL (see $OUT/log)" >&2; exit 1; }
ffmpeg -v error -y -i "$OUT/raw.mp4" -vf "setpts=PTS/$SPEED,fps=30,scale=1920:1200:flags=lanczos,format=yuv420p" \
  -c:v libx264 -profile:v high -preset slow -crf 17 -movflags +faststart -an "target/demo/$NAME.mp4"
echo "target/demo/$NAME.mp4 ($(ffprobe -v error -show_entries format=duration -of csv=p=0 "target/demo/$NAME.mp4") s)"
