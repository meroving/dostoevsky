#!/usr/bin/env bash
# Fails if a binary names private Apple API (dsky-a8n.19).
#
#   scripts/check_private_api.sh target/release/dostoevsky
#
# Rust's Objective-C bindings register selectors at run time from plain strings, so a private
# selector shows up as text in the binary: `_zoomFill:`, `_windowRestorationOptions`. This looks
# for that shape (underscore, a lowercase word, then camel case) and for private class names we
# have met. vendor/gpui-pre-macos has them taken out; a GPUI upgrade can bring them back.
set -euo pipefail
bin="${1:?usage: check_private_api.sh <binary>}"

found="$(strings -a "$bin" \
  | grep -E '^_[a-z]{3,}[A-Z][A-Za-z]*(:([A-Za-z]+:)*)?$|^NSWindowRestorationOptions$' \
  | sort -u || true)"
if [ -n "$found" ]; then
  echo "private API named in $bin (see vendor/gpui-pre-macos/PATCHES.md):" >&2
  echo "$found" | sed 's/^/  /' >&2
  exit 1
fi
echo "no private API names in $bin"
