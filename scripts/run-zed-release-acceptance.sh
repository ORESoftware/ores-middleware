#!/usr/bin/env bash
set -euo pipefail

readonly ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
readonly ZED_BIN="${ZED_BIN:-zed}"
readonly RECEIPT_DIR="${ZED_ACCEPTANCE_RECEIPT_DIR:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}/ores-zed-acceptance}"
readonly SOURCE_COMMIT="${ZED_ACCEPTANCE_SOURCE_COMMIT:-$(git -C "$ROOT" rev-parse HEAD)}"
readonly FIRST_PACK_DIR="${RECEIPT_DIR}/package-first"
readonly SECOND_PACK_DIR="${RECEIPT_DIR}/package-second"
readonly EXPECTED_ARCHIVES="${RECEIPT_DIR}/expected-archives.txt"
readonly FIRST_ARCHIVE_NAMES="${RECEIPT_DIR}/package-first-names.txt"
readonly SECOND_ARCHIVE_NAMES="${RECEIPT_DIR}/package-second-names.txt"
readonly FIRST_ARCHIVE_DIGESTS="${RECEIPT_DIR}/package-first-digests.txt"
readonly SECOND_ARCHIVE_DIGESTS="${RECEIPT_DIR}/package-second-digests.txt"
readonly RECEIPT="${RECEIPT_DIR}/receipt.json"
readonly SCOPE_RECEIPT="${RECEIPT_DIR}/acceptance-scope.json"

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

cd "$ROOT"
readonly PACKAGE_VERSION="$(python3 - <<'PY'
import re
import tomllib
from pathlib import Path
version = tomllib.loads(Path('.zpkg.toml').read_text())['package']['version']
if re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', version) is None:
    raise SystemExit(f'invalid stable package version: {version!r}')
print(version)
PY
)"
readonly REPOSITORY_ARCHIVE_NAME="oresoftware-ores-middleware-${PACKAGE_VERSION}.tar.gz"
readonly FIRST_REPOSITORY_ARCHIVE="${FIRST_PACK_DIR}/${REPOSITORY_ARCHIVE_NAME}"
readonly SECOND_REPOSITORY_ARCHIVE="${SECOND_PACK_DIR}/${REPOSITORY_ARCHIVE_NAME}"

mkdir -p "$RECEIPT_DIR"
rm -rf "$FIRST_PACK_DIR" "$SECOND_PACK_DIR"
rm -f \
  "$RECEIPT" \
  "$SCOPE_RECEIPT" \
  "$EXPECTED_ARCHIVES" \
  "$FIRST_ARCHIVE_NAMES" \
  "$SECOND_ARCHIVE_NAMES" \
  "$FIRST_ARCHIVE_DIGESTS" \
  "$SECOND_ARCHIVE_DIGESTS"

{
  printf 'oresoftware-ores-middleware-%s.tar.gz\n' "$PACKAGE_VERSION"
  printf 'oresoftware-ores-middleware-elixir-%s.tar.gz\n' "$PACKAGE_VERSION"
  printf 'oresoftware-ores-middleware-erlang-%s.tar.gz\n' "$PACKAGE_VERSION"
  printf 'oresoftware-ores-middleware-gleam-%s.tar.gz\n' "$PACKAGE_VERSION"
  printf 'oresoftware-ores-middleware-golang-%s.tar.gz\n' "$PACKAGE_VERSION"
  printf 'oresoftware-ores-middleware-rust-%s.tar.gz\n' "$PACKAGE_VERSION"
  printf 'oresoftware-ores-middleware-typescript-%s.tar.gz\n' "$PACKAGE_VERSION"
} | LC_ALL=C sort >"$EXPECTED_ARCHIVES"

# Reproducibility acceptance is intentionally credential-free. A dependency-
# bearing package cannot use Zed's isolated file-registry r2g unless every
# dependency is first mirrored into that throwaway registry. Registry closure
# is therefore verified separately by the release workflow after publication.
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
  find "$directory" -mindepth 1 -maxdepth 1 -type f -name '*.tar.gz' -printf '%f\n' |
    LC_ALL=C sort
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

zed_clean_env "$ZED_BIN" validate --json >"${RECEIPT_DIR}/validate.json"
zed_clean_env "$ZED_BIN" pack --out "$FIRST_PACK_DIR"
zed_clean_env "$ZED_BIN" pack --out "$SECOND_PACK_DIR"

archive_names "$FIRST_PACK_DIR" >"$FIRST_ARCHIVE_NAMES"
archive_names "$SECOND_PACK_DIR" >"$SECOND_ARCHIVE_NAMES"
diff -u "$EXPECTED_ARCHIVES" "$FIRST_ARCHIVE_NAMES"
diff -u "$EXPECTED_ARCHIVES" "$SECOND_ARCHIVE_NAMES"

test -f "$FIRST_REPOSITORY_ARCHIVE"
test -f "$SECOND_REPOSITORY_ARCHIVE"
archive_digests "$FIRST_PACK_DIR" "$EXPECTED_ARCHIVES" >"$FIRST_ARCHIVE_DIGESTS"
archive_digests "$SECOND_PACK_DIR" "$EXPECTED_ARCHIVES" >"$SECOND_ARCHIVE_DIGESTS"
diff -u "$FIRST_ARCHIVE_DIGESTS" "$SECOND_ARCHIVE_DIGESTS"

cargo run --quiet --locked \
  --manifest-path tools/zed-archive-audit/Cargo.toml -- \
  --first "$FIRST_REPOSITORY_ARCHIVE" \
  --second "$SECOND_REPOSITORY_ARCHIVE" \
  --source-commit "$SOURCE_COMMIT" \
  --zed-version "$ZED_VERSION" \
  --receipt "$RECEIPT"

jq -e '
  .status == "passed" and
  .byteReproducible == true and
  (.archive.archiveSha256 | length) == 64 and
  (.archive.treeSha256 | length) == 64 and
  (.archive.requiredEntries | length) == 8
' "$RECEIPT" >/dev/null

python3 - "$SCOPE_RECEIPT" "$PACKAGE_VERSION" "$SOURCE_COMMIT" <<'PY'
import json
import sys
from pathlib import Path
path, version, commit = sys.argv[1:]
Path(path).write_text(json.dumps({
    'schema': 'ores.middleware.zed-acceptance-scope/v1',
    'status': 'passed',
    'packageVersion': version,
    'sourceCommit': commit,
    'credentialFreeReproducibility': True,
    'archiveSetClosed': True,
    'byteReproducible': True,
    'registryDependencyClosure': 'release-workflow',
    'reason': 'dependency-bearing packages require the authenticated registry for a complete consumer graph',
}, indent=2, sort_keys=True) + '\n')
PY

printf 'Zed reproducibility acceptance receipt: %s\n' "$RECEIPT"
printf 'Registry closure gate is delegated to the tag release workflow.\n'
