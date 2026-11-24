#!/usr/bin/env bash
# Records a demo video from the real app: scripts/demo/<name>.jsonl (with <name>.prefs.json)
# in a copy of the book scripts/demo/crime-and-punishment, through the shots runner's `record` step. AI talks to scripts/readme/stub_ai.py: no key, no network.
#
#   scripts/demo_video.sh [name]     → target/demo/<name>.mp4 (1920×1200, H.264; default: launch)
#
# Needs macOS with a GPU and ffmpeg. SPEED (default 1.1) speeds the take up in the final encode.
set -euo pipefail
export DOSTOEVSKY_LANG="${DOSTOEVSKY_LANG:-en}"
cd "$(dirname "$0")/.."
NAME="${1:-launch}"
SPEED="${SPEED:-1.1}"
PORT="${STUB_PORT:-11434}"   # another port when a real Ollama holds 11434
cargo build -q --release -p dostoevsky --features shots
OUT="$PWD/target/demo/$NAME"
rm -rf "$OUT" && mkdir -p "$OUT/support"
python3 -I scripts/readme/stub_ai.py "$PORT" >"$OUT/stub_ai.log" 2>&1 &
stub=$!
trap 'kill $stub 2>/dev/null || true' EXIT
sed "s#\$PORT#$PORT#g" "scripts/demo/$NAME.prefs.json" > "$OUT/support/prefs.json"
sed "s#\$OUT#$OUT#g" "scripts/demo/$NAME.jsonl" > "$OUT/steps.jsonl"
book="$OUT/crime-and-punishment"
cp -R scripts/demo/crime-and-punishment "$book"
sleep 0.5
DOSTOEVSKY_SECRETS=memory DOSTOEVSKY_WINDOW_SIZE="${WINDOW:-1280x800}" DOSTOEVSKY_SUPPORT_DIR="$OUT/support" \
  DOSTOEVSKY_SCRIPT="$OUT/steps.jsonl" timeout 180 target/release/dostoevsky "$book" >"$OUT/log" 2>&1 \
  || { echo "FAIL (see $OUT/log)" >&2; exit 1; }
ffmpeg -v error -y -i "$OUT/raw.mp4" -vf "setpts=PTS/$SPEED,fps=30,scale=1920:1200:flags=lanczos" \
  -c:v libx264 -preset fast -crf 10 -pix_fmt yuv444p "$OUT/take.mp4"
# The end card: the icon, the name in the headings' hand (Bad Script) and the site, on the
# Manuscript paper; the take fades into it.
fonts=crates/app/assets/fonts
magick -size 1920x1200 xc:'#EFE3CA' \
  \( packaging/icon-1024.png -resize 300x300 \) -gravity center -geometry +0-170 -composite \
  -fill '#3B2C20' -font "$fonts/BadScript-Regular.ttf" -pointsize 130 -annotate +0+95 'Dostoevsky' \
  -fill '#8A7660' -font "$fonts/IBMPlexMono-Regular.ttf" -pointsize 38 -annotate +0+235 'dostoevsky.app' \
  -fill '#A8957D' -pointsize 26 -annotate +0+295 'free and open source · for macOS' \
  "$OUT/end.png"
take=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$OUT/take.mp4")
fade=0.8
ffmpeg -v error -y -i "$OUT/take.mp4" -loop 1 -framerate 30 -t 3.2 -i "$OUT/end.png" \
  -filter_complex "[1:v]format=yuv444p,fps=30,settb=1/30,setsar=1[e];[0:v]fps=30,settb=1/30,setsar=1[t];[t][e]xfade=transition=fade:duration=$fade:offset=$(echo "$take - $fade" | bc),format=yuv420p" \
  -c:v libx264 -profile:v high -preset slow -crf 17 -r 30 -movflags +faststart -an "target/demo/$NAME.mp4"
echo "target/demo/$NAME.mp4 ($(ffprobe -v error -show_entries format=duration -of csv=p=0 "target/demo/$NAME.mp4") s)"
