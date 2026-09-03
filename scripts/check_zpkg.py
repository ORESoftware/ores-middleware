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
POLYGLOT_COMMAND = (
    "cargo run --quiet --manifest-path tools/contract-parity/Cargo.toml "
    "--bin persistence_codegen -- --output-root target/schema-convergence "
    "--report target/schema-convergence/receipt.json"
)


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
        "orm-catalog",
        "polyglot-generation",
        "projection-parity",
        "test",
        "zpkg-check",
    ):
        if required not in scripts:
            errors.append(f"missing scripts.{required}")
    if scripts.get("orm-catalog") != "python3 scripts/orm_matrix_gate.py":
        errors.append("scripts.orm-catalog must execute the four-way database-backed gate")
    for key in ("polyglot-generation", "projection-parity"):
        if scripts.get(key) != POLYGLOT_COMMAND:
            errors.append(
                f"scripts.{key} must execute the Rust independent polyglot generator"
            )

    required_paths = (
        "tools/contract-parity/src/bin/persistence_codegen.rs",
        "scripts/validate-generated-polyglot.mjs",
        "scripts/orm_matrix_gate.py",
        "scripts/test_orm_matrix_gate.py",
        "scripts/orm_catalog_gate.py",
        "scripts/subprocess_capture.py",
        ".github/workflows/persistence-convergence.yml",
    )
    for required_path in required_paths:
        if not (root / required_path).is_file():
            errors.append(f"missing required convergence gate file: {required_path}")

    smoke_test = manifest.get("publish", {}).get("smoke_test", "")
    for required_fragment in (
        "persistence_codegen",
        "validate-generated-polyglot.mjs",
        "scripts/cross_translate.py",
    ):
        if required_fragment not in smoke_test:
            errors.append(f"publish.smoke_test must execute {required_fragment}")

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
    required_gates = {
        "cross-translation-witnesses",
        "round-trip-witnesses",
        "sql-catalog-readback-when-applicable",
        "diesel-seaorm-catalog-parity-when-applicable",
    }
    if not required_gates.issubset(gates):
        errors.append(
            "authority topology must require translation, round-trip, SQL catalog, "
            "and Diesel/SeaORM gates"
        )
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
        "golang + gleam + elixir + erlang + independent TypeSpec/JSON-Schema "
        "SQL/type/runtime generation + bidirectional shadow gate + four-way "
        "Diesel/SeaORM database-backed convergence"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
