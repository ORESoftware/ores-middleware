#!/usr/bin/env python3
"""Validate the repository's zed-pkg polyglot publication contract."""

from __future__ import annotations

import argparse
import json
import tomllib
from pathlib import Path

EXPECTED_TARGETS = {
    "repository": (".", "none", None),
    "rust": ("src/rust", "rust", "ores-middleware-rust"),
    "typescript": ("src/ts", "node", "ores-middleware-typescript"),
    "golang": ("src/golang", "go", "ores-middleware-golang"),
    "gleam": ("src/gleam", "none", "ores-middleware-gleam"),
    "elixir": ("src/elixir", "none", "ores-middleware-elixir"),
    "erlang": ("src/erlang", "none", "ores-middleware-erlang"),
}
EXPECTED_OUTPUTS = {
    "target/rust",
    "target/ts",
    "target/golang",
    "target/gleam",
    "target/elixir",
    "target/erlang",
}
EXPECTED_FLOWS = {
    "typespec": ["sql-when-applicable", "protobuf", "grpc", "wire-clients"],
    "json-schema-openapi": ["interfaces-types", "sql-when-applicable", "write-clients"],
}


def validate(root: Path) -> list[str]:
    errors: list[str] = []
    manifest = tomllib.loads((root / ".zpkg.toml").read_text(encoding="utf-8"))
    package = manifest.get("package", {})
    if package.get("org") != "oresoftware" or package.get("name") != "ores-middleware":
        errors.append("package identity must be oresoftware/ores-middleware")
    if "language" in package:
        errors.append("package.language must remain unset for a polyglot repository")

    targets = manifest.get("targets", {})
    if set(targets) != set(EXPECTED_TARGETS):
        errors.append(f"targets must be exactly {sorted(EXPECTED_TARGETS)}, got {sorted(targets)}")
    names: set[str] = set()
    for key, (directory, adapter, name) in EXPECTED_TARGETS.items():
        actual = targets.get(key, {})
        if actual.get("dir") != directory:
            errors.append(f"targets.{key}.dir must be {directory!r}")
        if actual.get("adapter") != adapter:
            errors.append(f"targets.{key}.adapter must be {adapter!r}")
        if name is None:
            if "name" in actual:
                errors.append("targets.repository must publish under package.name and omit name")
        elif actual.get("name") != name:
            errors.append(f"targets.{key}.name must be {name!r}")
        elif name in names:
            errors.append(f"duplicate target package name {name!r}")
        else:
            names.add(name)
        if directory != "." and not (root / directory).is_dir():
            errors.append(f"target directory does not exist: {directory}")

    build = manifest.get("build", {})
    if build.get("command") != "python3 scripts/build_targets.py":
        errors.append("build.command must use the checked-in polyglot build orchestrator")
    outputs = set(build.get("outputs", []))
    if outputs != EXPECTED_OUTPUTS:
        errors.append(f"build.outputs mismatch: expected {sorted(EXPECTED_OUTPUTS)}, got {sorted(outputs)}")

    scripts = manifest.get("scripts", {})
    for required in (
        "audit",
        "build",
        "contracts",
        "cross-translation",
        "projection-parity",
        "test",
        "zpkg-check",
    ):
        if required not in scripts:
            errors.append(f"missing scripts.{required}")
    smoke_test = manifest.get("publish", {}).get("smoke_test", "")
    if "scripts/cross_translate.py" not in smoke_test:
        errors.append("publish.smoke_test must execute the cross-translation gate")

    topology = json.loads((root / "contracts/authority-topology.json").read_text(encoding="utf-8"))
    authorities = {
        item.get("id")
        for item in topology.get("authorities", [])
        if item.get("kind") == "human-authored" and item.get("topLevel") is True
    }
    if authorities != {"typespec", "json-schema-openapi"}:
        errors.append("authority topology must retain TypeSpec and JSON Schema/OpenAPI as top-level peers")
    if topology.get("flows") != EXPECTED_FLOWS:
        errors.append("authority topology flow mismatch")
    gates = set(topology.get("convergenceGates", []))
    if not {"cross-translation-witnesses", "round-trip-witnesses"}.issubset(gates):
        errors.append("authority topology must require cross-translation and round-trip witnesses")
    if topology.get("onUnexplainedMismatch") != "STOPPED_FOR_EVALUATION":
        errors.append("unexplained mismatches must stop for evaluation")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    errors = validate(args.root.resolve())
    if errors:
        print("invalid .zpkg.toml:")
        for item in errors:
            print(f"- {item}")
        return 1
    print(
        ".zpkg.toml polyglot contract passed: repository + rust + typescript + "
        "golang + gleam + elixir + erlang + bidirectional shadow gate"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
