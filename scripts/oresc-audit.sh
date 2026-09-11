#!/usr/bin/env bash
set -euo pipefail

ORESC_BIN="${ORESC_BIN:-oresc}"
REPORT_DIR="${ORESC_REPORT_DIR:-target/audit}"

if ! command -v "$ORESC_BIN" >/dev/null 2>&1; then
  echo "oresc is required; install the canonical ORESoftware/ores-cli package before running this audit" >&2
  exit 70
fi

mkdir -p "$REPORT_DIR"

echo "[oresc] repository standards"
"$ORESC_BIN" --no-json audit repo --path . --profile standards

echo "[oresc] docs-serving TypeSpec / JSON Schema peer-authority admission"
"$ORESC_BIN" --no-json audit contract \
  --typespec contracts/docs-serving.tsp \
  --schema contracts/docs-serving.schema.json \
  --report "$REPORT_DIR/oresc-docs-serving.json"

# Root Cargo.toml is a workspace-only manifest. The current package audit models
# one concrete Cargo package, so root package synchronization remains owned by
# scripts/check_zpkg.py until ores-cli gains explicit workspace-package support.
