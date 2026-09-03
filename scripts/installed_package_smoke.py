#!/usr/bin/env python3
"""Execute the self-contained Zed installation produced by this repository.

The smoke test runs only files declared as build outputs. It proves that the
installed root package retains both authored persistence contracts, can rerun
the non-authoritative cross-translation witness, and exposes executable SDK
artifacts after the source checkout has disappeared.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any

EXPECTED_TARGETS = ("rust", "ts", "golang", "gleam", "elixir", "erlang")
EXPECTED_LANGUAGES = {"rust", "ts", "golang"}
EXPECTED_NODE_RUNTIME_PACKAGE = "node_modules/@oresoftware/next-loggers"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(command: list[str], *, cwd: Path | None = None) -> str:
    completed = subprocess.run(
        command,
        cwd=cwd,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=120,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"installed-package command failed ({completed.returncode}): "
            f"{' '.join(command)}\n{completed.stdout}"
        )
    return completed.stdout.strip()


def run_json(command: list[str]) -> dict[str, Any]:
    output = run(command)
    try:
        value = json.loads(output)
    except json.JSONDecodeError as exc:
        raise ValueError(f"command did not emit one JSON document: {command}: {output}") from exc
    if not isinstance(value, dict):
        raise ValueError(f"descriptor must be a JSON object: {command}")
    return value


def descriptor_view(value: dict[str, Any], expected_language: str) -> dict[str, Any]:
    language = value.get("language")
    if language != expected_language:
        raise ValueError(
            f"descriptor language mismatch: expected {expected_language!r}, got {language!r}"
        )
    contract_version = value.get("contractVersion")
    capabilities = value.get("capabilities")
    operations = value.get("operationSymbols")
    if not isinstance(contract_version, str) or not contract_version:
        raise ValueError(f"{expected_language} descriptor lacks contractVersion")
    if not isinstance(capabilities, list) or not all(
        isinstance(item, str) and item for item in capabilities
    ):
        raise ValueError(f"{expected_language} descriptor lacks string capabilities")
    if not isinstance(operations, dict) or not all(
        isinstance(key, str)
        and key
        and isinstance(symbol, str)
        and symbol
        for key, symbol in operations.items()
    ):
        raise ValueError(f"{expected_language} descriptor lacks operationSymbols")
    return {
        "contractVersion": contract_version,
        "capabilities": capabilities,
        "operationKeys": sorted(operations),
    }


def find_one(root: Path, pattern: str) -> Path:
    candidates = sorted(path for path in root.rglob(pattern) if path.is_file())
    if len(candidates) != 1:
        raise ValueError(
            f"expected exactly one {pattern!r} below {root}, found "
            f"{[str(path) for path in candidates]}"
        )
    return candidates[0]


def assert_self_contained(root: Path) -> None:
    resolved_root = root.resolve(strict=True)
    for path in sorted(root.rglob("*")):
        if not path.is_symlink():
            continue
        relative = path.relative_to(root)
        try:
            resolved = path.resolve(strict=True)
        except FileNotFoundError as exc:
            raise ValueError(f"installed package contains broken symlink: {relative}") from exc
        if not resolved.is_relative_to(resolved_root):
            raise ValueError(
                f"installed package symlink escapes root: {relative} -> {resolved}"
            )


def assert_node_runtime_closure(typescript_root: Path) -> None:
    lock_path = typescript_root / "package-lock.json"
    receipt_path = typescript_root / "runtime-dependencies.json"
    if not lock_path.is_file():
        raise ValueError(f"installed TypeScript lock is missing: {lock_path}")
    if not receipt_path.is_file():
        raise ValueError(f"installed Node runtime closure receipt is missing: {receipt_path}")

    receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
    if receipt.get("schema") != "ores.node-runtime-dependency-closure/v1":
        raise ValueError("installed Node runtime closure has an unsupported schema")
    if receipt.get("packageLockSha256") != sha256(lock_path):
        raise ValueError("installed Node runtime closure lock digest drifted")

    packages = receipt.get("packages")
    if not isinstance(packages, list) or not all(isinstance(item, dict) for item in packages):
        raise ValueError("installed Node runtime closure lacks package records")
    paths = [item.get("path") for item in packages]
    if EXPECTED_NODE_RUNTIME_PACKAGE not in paths:
        raise ValueError(
            "installed Node runtime closure omitted the pinned logging dependency: "
            f"{paths!r}"
        )

    dependency = typescript_root / EXPECTED_NODE_RUNTIME_PACKAGE / "package.json"
    if not dependency.is_file():
        raise ValueError(f"installed Node runtime dependency is missing: {dependency}")


def assert_typescript_runtime(typescript_root: Path) -> dict[str, Any]:
    entries = {
        "index": typescript_root / "dist" / "index.js",
        "operation": typescript_root / "dist" / "operation.js",
        "otel": typescript_root / "dist" / "otel.js",
    }
    for label, entry in entries.items():
        if not entry.is_file():
            raise ValueError(f"installed TypeScript {label} entry is missing: {entry}")

    run(
        [
            "node",
            "--input-type=module",
            "--eval",
            (
                "const root = await import(process.argv[1]); "
                "const operation = await import(process.argv[2]); "
                "const otel = await import(process.argv[3]); "
                "if (typeof root.descriptor !== 'function' || "
                "typeof operation.runOperationBoundary !== 'function' || "
                "typeof otel.createOresOtelMiddleware !== 'function') process.exit(2);"
            ),
            entries["index"].resolve().as_uri(),
            entries["operation"].resolve().as_uri(),
            entries["otel"].resolve().as_uri(),
        ]
    )
    return run_json(
        [
            "node",
            "--input-type=module",
            "--eval",
            (
                "const module = await import(process.argv[1]); "
                "process.stdout.write(JSON.stringify(module.descriptor()));"
            ),
            entries["index"].resolve().as_uri(),
        ]
    )


def assert_beam_runtime(target_root: Path) -> None:
    gleam_beam = find_one(target_root / "gleam" / "build", "ores_middleware.beam")
    erlang_beam = find_one(
        target_root / "erlang" / "_build" / "default" / "lib" / "ores_middleware",
        "ores_middleware.beam",
    )
    elixir_beam = find_one(target_root / "elixir" / "_build", "Elixir.OresMiddleware.beam")

    for label, beam in (("gleam", gleam_beam), ("erlang", erlang_beam)):
        output = run(
            [
                "erl",
                "-noshell",
                "-pa",
                str(beam.parent),
                "-eval",
                (
                    "case ores_middleware:descriptor() of "
                    "undefined -> halt(2); _ -> io:format(\"ok\"), halt(0) end."
                ),
            ]
        )
        if output != "ok":
            raise ValueError(f"{label} installed descriptor probe returned {output!r}")

    output = run(
        [
            "elixir",
            "-pa",
            str(elixir_beam.parent),
            "-e",
            (
                "case OresMiddleware.descriptor() do "
                "%{} -> IO.write(\"ok\"); _ -> System.halt(2) end"
            ),
        ]
    )
    if output != "ok":
        raise ValueError(f"elixir installed descriptor probe returned {output!r}")


def validate(root: Path) -> None:
    root = root.resolve(strict=True)
    target_root = root / "target"
    evidence_root = target_root / "package"
    manifest_path = evidence_root / "manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("schema") != "ores.zed-installed-package/v1":
        raise ValueError("installed evidence manifest has an unsupported schema")
    if manifest.get("targets") != list(EXPECTED_TARGETS):
        raise ValueError("installed evidence manifest target set/order drifted")

    source_digests = manifest.get("sourceDigests")
    if not isinstance(source_digests, dict) or not source_digests:
        raise ValueError("installed evidence manifest lacks sourceDigests")
    for relative, expected in sorted(source_digests.items()):
        path = evidence_root / relative
        if not path.is_file():
            raise ValueError(f"installed authority evidence is missing: {relative}")
        actual = sha256(path)
        if actual != expected:
            raise ValueError(
                f"installed authority evidence digest drifted: {relative}: "
                f"expected {expected}, got {actual}"
            )

    for name in EXPECTED_TARGETS:
        path = target_root / name
        if not path.is_dir():
            raise ValueError(f"installed language target is missing: target/{name}")

    assert_self_contained(root)

    cross_translate = evidence_root / "scripts" / "cross_translate.py"
    run([sys.executable, str(cross_translate), "--root", str(evidence_root)])

    rust_binary = target_root / "rust" / "debug" / "contractcheck"
    go_binary = target_root / "golang" / "contractcheck"
    typescript_root = target_root / "ts"
    for binary in (rust_binary, go_binary):
        if not binary.is_file() or not os.access(binary, os.X_OK):
            raise ValueError(f"installed executable is missing or not executable: {binary}")

    assert_node_runtime_closure(typescript_root)
    descriptors = {
        "rust": run_json([str(rust_binary)]),
        "golang": run_json([str(go_binary)]),
        "ts": assert_typescript_runtime(typescript_root),
    }
    if set(descriptors) != EXPECTED_LANGUAGES:
        raise AssertionError("internal descriptor probe set drifted")
    normalized = {
        language: descriptor_view(value, language)
        for language, value in descriptors.items()
    }
    baseline = normalized["rust"]
    for language in ("golang", "ts"):
        if normalized[language] != baseline:
            raise ValueError(
                f"installed descriptor semantics diverged: rust={baseline!r}; "
                f"{language}={normalized[language]!r}"
            )

    assert_beam_runtime(target_root)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    args = parser.parse_args()
    validate(args.root)
    print(
        "installed Zed package passed: copied peer authorities, cross-translation, "
        "locked Node dependency closure, Rust/TypeScript/Go descriptor parity, "
        "and Gleam/Elixir/Erlang runtime probes"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
