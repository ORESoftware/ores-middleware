#!/usr/bin/env python3
"""Fail-closed ownership audit for repository configuration surfaces.

`.zpkg.toml` is package/build/publish metadata only. Middleware runtime policy and
orchestration must not be nested into it. `.ores-mw.toml` remains the middleware
orchestration/env-metadata manifest and is normalized by the existing peer-
authority-backed parser.

This audit intentionally does not invent a second middleware schema. Detailed
provider policy remains in the owning config families (`.auth-shared.toml`,
`.ores-rl.toml`, `.ores-lru.toml`, `.ores-otel.toml`) or in the referenced
peer-authority middleware stack JSON.
"""
from __future__ import annotations

import argparse
import json
import sys
import tomllib
from pathlib import Path
from typing import Any, Iterable

# Running this file directly places scripts/ on sys.path, so this import resolves
# the canonical `.ores-mw.toml` parser rather than duplicating its schema here.
from ores_mw_config import ManifestError, normalize_manifest

MAX_CONFIG_BYTES = 256 * 1024
FORBIDDEN_ZPKG_SEGMENTS = frozenset({
    "middleware",
    "middleware_runtime",
    "runtime_middleware",
})


class OwnershipError(ValueError):
    def __init__(self, code: str, field: str = "") -> None:
        super().__init__(code)
        self.code = code
        self.field = field


def _need(ok: bool, code: str, field: str = "") -> None:
    if not ok:
        raise OwnershipError(code, field)


def _read_toml(path: Path, *, label: str) -> dict[str, Any]:
    try:
        stat = path.lstat()
    except OSError as exc:
        raise OwnershipError(f"{label}-unavailable", path.name) from exc
    _need(path.is_file() and not path.is_symlink(), f"{label}-regular-file-required", path.name)
    _need(stat.st_size <= MAX_CONFIG_BYTES, f"{label}-too-large", path.name)
    try:
        data = path.read_bytes()
    except OSError as exc:
        raise OwnershipError(f"{label}-unavailable", path.name) from exc
    _need(len(data) <= MAX_CONFIG_BYTES, f"{label}-too-large", path.name)
    try:
        value = tomllib.loads(data.decode("utf-8"))
    except (UnicodeDecodeError, tomllib.TOMLDecodeError) as exc:
        raise OwnershipError(f"invalid-{label}-toml", path.name) from exc
    _need(isinstance(value, dict), f"{label}-object-required", path.name)
    return value


def _canonical_segment(key: str) -> str:
    return key.strip().lower().replace("-", "_")


def _audit_zpkg_tree(value: Any, path: tuple[str, ...] = ()) -> None:
    if isinstance(value, dict):
        for raw_key, nested in value.items():
            key = str(raw_key)
            segment = _canonical_segment(key)
            next_path = (*path, key)
            if segment in FORBIDDEN_ZPKG_SEGMENTS:
                raise OwnershipError(
                    "middleware-runtime-config-forbidden-in-zpkg",
                    ".".join(next_path),
                )
            _audit_zpkg_tree(nested, next_path)
    elif isinstance(value, list):
        for index, nested in enumerate(value):
            _audit_zpkg_tree(nested, (*path, f"[{index}]"))


def audit_repository(repo_root: Path) -> dict[str, Any]:
    try:
        root = repo_root.resolve(strict=True)
    except OSError as exc:
        raise OwnershipError("repository-unavailable", str(repo_root)) from exc
    _need(root.is_dir(), "repository-directory-required", str(repo_root))

    zpkg = _read_toml(root / ".zpkg.toml", label="zpkg")
    _audit_zpkg_tree(zpkg)

    raw_middleware = _read_toml(root / ".ores-mw.toml", label="ores-mw")
    try:
        normalized = normalize_manifest(raw_middleware)
    except ManifestError as exc:
        # Preserve the canonical parser's bounded error code/field without
        # echoing TOML values into logs or CI output.
        raise OwnershipError(f"ores-mw-{exc.code}", exc.field) from exc

    targets = normalized.get("targets", [])
    _need(isinstance(targets, list) and bool(targets), "ores-mw-target-required", "targets")

    return {
        "schema": "ores.middleware.config-ownership-audit/v1",
        "status": "passed",
        "zpkgRuntimeMiddlewareTables": 0,
        "middlewareTargetCount": len(targets),
        "flags2envBound": "flags2env" in normalized,
    }


def _write_json(value: dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n")


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(prog="check-config-ownership")
    result.add_argument("--repo-root", default=".")
    return result


def main(argv: Iterable[str] | None = None) -> int:
    args = parser().parse_args(list(argv) if argv is not None else None)
    try:
        _write_json(audit_repository(Path(args.repo_root)))
        return 0
    except OwnershipError as exc:
        _write_json({"status": "failed", "code": exc.code, "field": exc.field})
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
