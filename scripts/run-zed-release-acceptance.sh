#!/usr/bin/env bash
set -euo pipefail

readonly ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
readonly ZED_BIN="${ZED_BIN:-zed}"
readonly RECEIPT_DIR="${ZED_ACCEPTANCE_RECEIPT_DIR:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}/ores-zed-acceptance}"
readonly SOURCE_COMMIT="${ZED_ACCEPTANCE_SOURCE_COMMIT:-$(git -C "$ROOT" rev-parse HEAD)}"
readonly R2G_ROOT="${RECEIPT_DIR}/r2g"
readonly FIRST_ARCHIVE="${RECEIPT_DIR}/package-first.tar.gz"
readonly SECOND_ARCHIVE="${RECEIPT_DIR}/package-second.tar.gz"
readonly RECEIPT="${RECEIPT_DIR}/receipt.json"

case "$SOURCE_COMMIT" in
  *[!0-9a-f]*|'')
    printf '%s\n' 'error: source commit must be lowercase hexadecimal' >&2
    exit 64
    ;;
esac
if [ "${#SOURCE_COMMIT}" -ne 40 ]; then
  printf '%s\n' 'error: source commit must be exactly 40 characters' >&2
  exit 64
fi

mkdir -p "$RECEIPT_DIR"
rm -rf "$R2G_ROOT"
rm -f "$FIRST_ARCHIVE" "$SECOND_ARCHIVE" "$RECEIPT"

# Isolated r2g does not require registry authentication. Remove credential and
# provider variables from every Zed subprocess so a hosted runner or developer
# shell cannot accidentally change the trust boundary or leak a value.
zed_clean_env() {
  env \
    -u ZED_PKG_TOKEN \
    -u ZED_PKG_AUTH_PASSWORD \
    -u ZED_PKG_SUPABASE_KEY \
    -u GITHUB_TOKEN \
    "$@"
}

readonly ZED_VERSION="$(zed_clean_env "$ZED_BIN" --version)"
printf '%s\n' "$ZED_VERSION" >"${RECEIPT_DIR}/zed-version.txt"

cd "$ROOT"
zed_clean_env "$ZED_BIN" validate --json >"${RECEIPT_DIR}/validate.json"
zed_clean_env "$ZED_BIN" pack --out "$FIRST_ARCHIVE"
zed_clean_env "$ZED_BIN" pack --out "$SECOND_ARCHIVE"

cargo run --quiet --locked \
  --manifest-path tools/zed-archive-audit/Cargo.toml -- \
  --first "$FIRST_ARCHIVE" \
  --second "$SECOND_ARCHIVE" \
  --source-commit "$SOURCE_COMMIT" \
  --zed-version "$ZED_VERSION" \
  --receipt "$RECEIPT"

zed_clean_env "$ZED_BIN" r2g \
  --registry-mode isolated \
  --r2g-root "$R2G_ROOT"

(
  cd "$R2G_ROOT"
  find . -type f -print0 \
    | LC_ALL=C sort -z \
    | xargs -0 -r sha256sum
) >"${RECEIPT_DIR}/r2g-installed-file-digests.txt"

jq -e '
  .status == "passed" and
  .byteReproducible == true and
  (.archive.archiveSha256 | length) == 64 and
  (.archive.treeSha256 | length) == 64 and
  (.archive.requiredEntries | length) == 8
' "$RECEIPT" >/dev/null

printf 'Zed release acceptance receipt: %s\n' "$RECEIPT"
