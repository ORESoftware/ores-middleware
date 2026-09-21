#!/usr/bin/env bash
set -euo pipefail
# The install hook is deterministic and local-only: admit the complete authored
# contracts/conformance/governance/source boundary before enabling Git hooks.
sh ./conformance/check.sh --full
hooks_path="$(git config --get core.hooksPath 2>/dev/null || true)"
case "$hooks_path" in
  "")
    git config core.hooksPath .githooks
    echo '[zed-conformance] enabled tracked Git hooks via core.hooksPath=.githooks'
    ;;
  .githooks) ;;
  *)
    echo "[zed-conformance] preserving existing core.hooksPath=$hooks_path; tracked .githooks/pre-push is not auto-enabled" >&2
    ;;
esac
