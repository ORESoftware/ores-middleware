#!/usr/bin/env bash
set -euo pipefail

readonly ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
readonly ZED_BIN="${ZED_BIN:-zed}"
readonly RECEIPT_DIR="${ZED_ACCEPTANCE_RECEIPT_DIR:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}/ores-zed-acceptance}"
readonly SOURCE_COMMIT="${ZED_ACCEPTANCE_SOURCE_COMMIT:-$(git -C "$ROOT" rev-parse HEAD)}"
readonly R2G_ROOT="${RECEIPT_DIR}/r2g"
readonly FIRST_PACK_DIR="${RECEIPT_DIR}/package-first"
readonly SECOND_PACK_DIR="${RECEIPT_DIR}/package-second"
readonly EXPECTED_ARCHIVES="${RECEIPT_DIR}/expected-archives.txt"
readonly FIRST_ARCHIVE_NAMES="${RECEIPT_DIR}/package-first-names.txt"
readonly SECOND_ARCHIVE_NAMES="${RECEIPT_DIR}/package-second-names.txt"
readonly FIRST_ARCHIVE_DIGESTS="${RECEIPT_DIR}/package-first-digests.txt"
readonly SECOND_ARCHIVE_DIGESTS="${RECEIPT_DIR}/package-second-digests.txt"
readonly REPOSITORY_ARCHIVE_NAME="oresoftware-ores-middleware-0.1.0.tar.gz"
readonly FIRST_REPOSITORY_ARCHIVE="${FIRST_PACK_DIR}/${REPOSITORY_ARCHIVE_NAME}"
readonly SECOND_REPOSITORY_ARCHIVE="${SECOND_PACK_DIR}/${REPOSITORY_ARCHIVE_NAME}"
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
rm -rf "$R2G_ROOT" "$FIRST_PACK_DIR" "$SECOND_PACK_DIR"
rm -f \
  "$RECEIPT" \
  "$EXPECTED_ARCHIVES" \
  "$FIRST_ARCHIVE_NAMES" \
  "$SECOND_ARCHIVE_NAMES" \
  "$FIRST_ARCHIVE_DIGESTS" \
  "$SECOND_ARCHIVE_DIGESTS"

cat >"$EXPECTED_ARCHIVES" <<'EOF'
oresoftware-ores-middleware-0.1.0.tar.gz
oresoftware-ores-middleware-elixir-0.1.0.tar.gz
oresoftware-ores-middleware-erlang-0.1.0.tar.gz
oresoftware-ores-middleware-gleam-0.1.0.tar.gz
oresoftware-ores-middleware-golang-0.1.0.tar.gz
oresoftware-ores-middleware-rust-0.1.0.tar.gz
oresoftware-ores-middleware-typescript-0.1.0.tar.gz
EOF

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

archive_names() {
  local directory="$1"
  find "$directory" \
    -mindepth 1 \
    -maxdepth 1 \
    -type f \
    -name '*.tar.gz' \
    -printf '%f\n' \
    | LC_ALL=C sort
}

archive_digests() {
  local directory="$1"
  local names_file="$2"
  (
    cd "$directory"
    while IFS= read -r archive; do
      sha256sum "$archive"
    done <"$names_file"
  )
}

readonly ZED_VERSION="$(zed_clean_env "$ZED_BIN" --version)"
printf '%s\n' "$ZED_VERSION" >"${RECEIPT_DIR}/zed-version.txt"

cd "$ROOT"
zed_clean_env "$ZED_BIN" validate --json >"${RECEIPT_DIR}/validate.json"
zed_clean_env "$ZED_BIN" pack --out "$FIRST_PACK_DIR"
zed_clean_env "$ZED_BIN" pack --out "$SECOND_PACK_DIR"

# Zed 0.2.3 emits one archive for the canonical repository package and one for
# each language target. Verify the closed expected set before comparing bytes;
# an omitted or unexpected target is a release discrepancy, not a warning.
archive_names "$FIRST_PACK_DIR" >"$FIRST_ARCHIVE_NAMES"
archive_names "$SECOND_PACK_DIR" >"$SECOND_ARCHIVE_NAMES"
diff -u "$EXPECTED_ARCHIVES" "$FIRST_ARCHIVE_NAMES"
diff -u "$EXPECTED_ARCHIVES" "$SECOND_ARCHIVE_NAMES"

test -f "$FIRST_REPOSITORY_ARCHIVE"
test -f "$SECOND_REPOSITORY_ARCHIVE"
archive_digests "$FIRST_PACK_DIR" "$EXPECTED_ARCHIVES" >"$FIRST_ARCHIVE_DIGESTS"
archive_digests "$SECOND_PACK_DIR" "$EXPECTED_ARCHIVES" >"$SECOND_ARCHIVE_DIGESTS"
diff -u "$FIRST_ARCHIVE_DIGESTS" "$SECOND_ARCHIVE_DIGESTS"

# The typed auditor deeply inspects the canonical whole-repository archive,
# which contains both peer authorities, all six language manifests, and the
# package contract. The digest comparison above independently proves that every
# emitted target archive is byte-reproducible across both builds.
cargo run --quiet --locked \
  --manifest-path tools/zed-archive-audit/Cargo.toml -- \
  --first "$FIRST_REPOSITORY_ARCHIVE" \
  --second "$SECOND_REPOSITORY_ARCHIVE" \
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
