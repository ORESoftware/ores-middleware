#!/bin/sh
set -eu
mode=${1:-full}
case "$mode" in
  full|--full) mode=full ;;
  structural|--structural-only) mode=structural ;;
  *) echo "usage: conformance/check.sh [--full|--structural-only]" >&2; exit 2 ;;
esac
root=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
cd "$root"
fail() { echo "[zed-conformance] $*" >&2; exit 1; }
for boundary in contracts conformance governance src; do
  [ ! -L "$boundary" ] || fail "$boundary must be a real directory, not a symbolic link"
  [ -d "$boundary" ] || fail "missing required top-level $boundary/ boundary"
done
# Inspect the authored repository boundary, not dependency/build output created by
# package managers during CI. Git mode 120000 is the authoritative signal for a
# committed symbolic link and cannot be confused by node_modules/target/_build.
escaped=$(git ls-files -s -- contracts conformance governance src | awk '$1 == "120000" { print $4; exit }')
[ -z "$escaped" ] || fail "symbolic links are not allowed inside tracked contract/conformance/governance/source boundaries: $escaped"
echo "[zed-conformance] structural boundary check passed"
[ "$mode" = full ] || exit 0
command -v node >/dev/null 2>&1 || fail "node is required to run conformance checks"
[ -f scripts/check-polyglot-governance.mjs ] || fail "missing polyglot governance runner scripts/check-polyglot-governance.mjs"
[ -f conformance/check.mjs ] || fail "missing mature conformance runner conformance/check.mjs"
node scripts/check-polyglot-governance.mjs
exec node conformance/check.mjs
