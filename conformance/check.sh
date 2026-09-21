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
escaped=$(find contracts conformance governance src -type l -print -quit 2>/dev/null || true)
[ -z "$escaped" ] || fail "symbolic links are not allowed inside contract/conformance/governance/source boundaries: $escaped"
echo "[zed-conformance] structural boundary check passed"
[ "$mode" = full ] || exit 0
command -v node >/dev/null 2>&1 || fail "node is required to run conformance checks"
[ -f scripts/check-polyglot-governance.mjs ] || fail "missing polyglot governance runner scripts/check-polyglot-governance.mjs"
[ -f conformance/check.mjs ] || fail "missing mature conformance runner conformance/check.mjs"
node scripts/check-polyglot-governance.mjs
exec node conformance/check.mjs
