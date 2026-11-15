#!/usr/bin/env bash
# NFR-1 performance budgets (PRD §8). Fails if any is over budget.
#
#   scripts/perf_check.sh               # budgets as in the PRD
#   PERF_SLACK=2 scripts/perf_check.sh  # every budget × 2, for slower or shared machines (CI)
#
# - Keystroke to paint and cycling (p99 at 20k words) and opening a 100k-word file: release
#   UI tests in crates/app/src/ui_tests/perf.rs (edit + index + layout + paint, no GPU).
# - Cold start to the first frame: the release app is launched (median of 3) with an empty
#   document; measured from exec to the first drawn frame (DOSTOEVSKY_PERF), after one
#   discarded launch (macOS checks a newly built executable once, ~1 s).
# A failed frame/open run is retried once before it counts.
set -euo pipefail
cd "$(dirname "$0")/.."
SLACK=${PERF_SLACK:-1}
export PERF_SLACK=$SLACK
fail=0

echo "== frame and open budgets (slack ×$SLACK)"
# One retry: a single run can be disturbed by other work on the machine.
ok=0
for attempt in 1 2; do
  out=$(cargo test -q --release -p dostoevsky ui_tests::perf -- --ignored --test-threads=1 --nocapture 2>&1) && ok=1
  echo "$out" | grep -oE 'perf: .*|.* is over the NFR-1 budget .*'
  [ $ok -eq 1 ] && break
  [ $attempt -eq 1 ] && echo "(over budget; retrying once)"
done
[ $ok -eq 1 ] || fail=1

echo "== cold start"
cargo build -q --release -p dostoevsky
SUPPORT="$PWD/target/perf-support"
rm -rf "$SUPPORT"; mkdir -p "$SUPPORT"
python3 - "$SLACK" "$SUPPORT" <<'PY' || fail=1
import os, re, statistics, subprocess, sys, time
slack, support = float(sys.argv[1]), sys.argv[2]
env = dict(os.environ, DOSTOEVSKY_PERF="1", DOSTOEVSKY_SUPPORT_DIR=support)
runs = []
# The first launch of a freshly built executable is ~1 s slower (macOS checks it once);
# that launch is discarded.
for i in range(4):
    start = time.time() * 1000
    p = subprocess.Popen(["target/release/dostoevsky"], env=env, stderr=subprocess.PIPE, text=True)
    first = None
    deadline = time.time() + 20
    for line in p.stderr:
        m = re.search(r"perf: first frame at (\d+) ms", line)
        if m:
            first = int(m.group(1)) - start
            break
        if time.time() > deadline:
            break
    p.kill()
    p.wait()
    if first is None:
        print("cold start: no first frame within 20 s")
        sys.exit(1)
    if i > 0:
        runs.append(first)
med = statistics.median(runs)
budget = 400 * slack
print(f"perf: cold start to first frame: {med:.0f} ms (runs {', '.join(f'{r:.0f}' for r in runs)}; budget {budget:.0f} ms)")
sys.exit(0 if med <= budget else 1)
PY
rm -rf "$SUPPORT"

if [ $fail -ne 0 ]; then echo "FAIL: over an NFR-1 budget"; else echo "ok: all NFR-1 budgets"; fi
exit $fail
