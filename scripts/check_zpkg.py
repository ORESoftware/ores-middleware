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
    "target/package",
}
EXPECTED_FLOWS = {
    "typespec": [
        "interfaces-types",
        "runtime-code",
        "sql-when-applicable",
        "diesel",
        "seaorm",
        "protobuf",
        "grpc",
        "wire-clients",
    ],
    "json-schema-openapi": [
        "interfaces-types",
        "runtime-code",
        "sql-when-applicable",
        "diesel",
        "seaorm",
        "openapi",
        "write-clients",
    ],
}
POLYGLOT_COMMAND = (
    "cargo run --quiet --manifest-path tools/contract-parity/Cargo.toml "
    "--bin persistence_codegen -- --output-root target/schema-convergence "
    "--report target/schema-convergence/receipt.json"
)
EXPECTED_ZED_TEST_SCRIPT = (
    "python3 scripts/audit.py --receipt target/audit/receipt.json && "
    "npm run contracts:polyglot-generate && npm run contracts:generated-check"
)
EXPECTED_INSTALLED_SMOKE_TEST = (
    'python3 "$ZED_PKG_TEST_TARGET/target/package/scripts/installed_package_smoke.py" '
    '--root "$ZED_PKG_TEST_TARGET"'
)
EXPECTED_WORKSPACE_SCRIPTS = {
    "audit": "python3 scripts/audit.py --receipt target/audit/receipt.json",
    "contracts:compile": "tsp compile contracts/typespec --output-dir target/contracts/typespec && tsp compile contracts/docs-serving.tsp --no-emit && tsp compile contracts/persistence/idempotency-record.tsp --no-emit && tsp compile contracts/rate-limit-v2/typespec --no-emit",
    "contracts:cross-translate": "python3 scripts/cross_translate.py",
    "contracts:polyglot-generate": POLYGLOT_COMMAND,
    "contracts:generated-check": "node scripts/validate-generated-polyglot.mjs",
    "persistence:catalog": "python3 scripts/orm_matrix_gate.py",
    "persistence:data-plane": "python3 scripts/orm_data_plane_gate.py",
    "persistence:check": "npm run persistence:catalog && npm run persistence:data-plane",
    "zpkg:check": "python3 scripts/check_zpkg.py",
}


def validate(root: Path) -> list[str]:
    errors: list[str] = []
    manifest = tomllib.loads((root / ".zpkg.toml").read_text(encoding="utf-8"))
    package = manifest.get("package", {})
    if package.get("org") != "oresoftware" or package.get("name") != "ores-middleware":
        errors.append("package identity must be oresoftware/ores-middleware")
    if "language" in package:
        errors.append("package.language must remain unset for a polyglot repository")

    # Zed 0.2.3 uses a closed ScriptsSection with exactly one supported hook:
    # `test`. Richer repository commands remain in package.json and Justfile.
    scripts = manifest.get("scripts", {})
    if set(scripts) != {"test"}:
        errors.append(
            "Zed 0.2.3 [scripts] must contain exactly the supported test hook"
        )
    if scripts.get("test") != EXPECTED_ZED_TEST_SCRIPT:
        errors.append(f"scripts.test must be {EXPECTED_ZED_TEST_SCRIPT!r}")

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

    workspace = json.loads((root / "package.json").read_text(encoding="utf-8"))
    workspace_scripts = workspace.get("scripts", {})
    for name, expected in EXPECTED_WORKSPACE_SCRIPTS.items():
        if workspace_scripts.get(name) != expected:
            errors.append(f"package.json scripts.{name} must be {expected!r}")

    compare_command = workspace_scripts.get("contracts:compare", "")
    for required_fragment in (
        "contracts:polyglot-generate",
        "contracts:generated-check",
        "contracts:cross-translate",
    ):
        if required_fragment not in compare_command:
            errors.append(
                f"package.json scripts.contracts:compare must execute {required_fragment}"
            )

    required_paths = (
        "tools/contract-parity/src/bin/persistence_codegen.rs",
        "scripts/validate-generated-polyglot.mjs",
        "scripts/orm_matrix_gate.py",
        "scripts/test_orm_matrix_gate.py",
        "scripts/orm_data_plane_gate.py",
        "scripts/test_orm_data_plane_gate.py",
        "scripts/orm_catalog_gate.py",
        "docs/orm-data-plane-convergence.md",
        "scripts/subprocess_capture.py",
        "scripts/build_targets.py",
        "scripts/cross_translate.py",
        "scripts/installed_package_smoke.py",
        ".github/workflows/persistence-convergence.yml",
        ".github/workflows/zed-release-acceptance.yml",
    )
    for required_path in required_paths:
        if not (root / required_path).is_file():
            errors.append(f"missing required convergence/package gate file: {required_path}")

    matrix_source = (root / "scripts/orm_matrix_gate.py").read_text(encoding="utf-8")
    for lane in (
        "typespec-diesel",
        "typespec-seaorm",
        "json-schema-openapi-diesel",
        "json-schema-openapi-seaorm",
    ):
        if lane not in matrix_source:
            errors.append(f"four-way ORM matrix must retain lane {lane!r}")
    for required_text in (
        "def resolve_cli_path(",
        "output_root = resolve_cli_path(",
        "report_path = resolve_cli_path(",
    ):
        if required_text not in matrix_source:
            errors.append(
                f"ORM matrix must root-anchor relative evidence paths: missing {required_text!r}"
            )

    compatibility_source = (root / "scripts/orm_catalog_gate.py").read_text(
        encoding="utf-8"
    )
    for required_text in (
        "from scripts.orm_matrix_gate import main as matrix_main",
        "return matrix_main()",
    ):
        if required_text not in compatibility_source:
            errors.append(
                "legacy ORM entrypoint must delegate to the complete "
                f"TypeSpec/JSON-Schema x Diesel/SeaORM matrix: missing {required_text!r}"
            )

    data_plane_source = (root / "scripts/orm_data_plane_gate.py").read_text(
        encoding="utf-8"
    )
    for lane in (
        "typespec-diesel",
        "typespec-seaorm",
        "json-schema-openapi-diesel",
        "json-schema-openapi-seaorm",
    ):
        if lane not in data_plane_source:
            errors.append(f"row-level ORM matrix must retain lane {lane!r}")
    for required_text in (
        "diesel::insert_into",
        "DeriveEntityModel",
        "execute_unprepared",
        "ores.orm-data-plane-convergence-report/v1",
        "orm-data-plane-negative-case-accepted",
        'tinyvec = {{ version = "=1.13.0"',
    ):
        if required_text not in data_plane_source:
            errors.append(
                "row-level gate must execute real Diesel and SeaORM paths for "
                f"both authorities: missing {required_text!r}"
            )

    persistence_workflow = (
        root / ".github/workflows/persistence-convergence.yml"
    ).read_text(encoding="utf-8")
    for required_text in (
        "scripts/test_orm_data_plane_gate.py",
        "python3 scripts/orm_matrix_gate.py",
        "python3 scripts/orm_data_plane_gate.py",
        "target/orm-data-plane-gate/receipt.json",
    ):
        if required_text not in persistence_workflow:
            errors.append(
                f"persistence CI must retain catalog and row-level gates: missing {required_text!r}"
            )

    smoke_test = manifest.get("publish", {}).get("smoke_test", "")
    if smoke_test != EXPECTED_INSTALLED_SMOKE_TEST:
        errors.append(
            "publish.smoke_test must execute the installed build-output closure, "
            f"expected {EXPECTED_INSTALLED_SMOKE_TEST!r}, got {smoke_test!r}"
        )
    installed_smoke_path = root / "scripts/installed_package_smoke.py"
    if installed_smoke_path.is_file():
        installed_smoke = installed_smoke_path.read_text(encoding="utf-8")
        for required_text in (
            "cross_translate.py",
            'target_root / "package"',
            "Rust/TypeScript/Go descriptor parity",
            "Gleam/Elixir/Erlang runtime probes",
        ):
            if required_text not in installed_smoke:
                errors.append(
                    f"installed package smoke test must retain {required_text!r}"
                )

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
        "diesel-seaorm-row-level-parity-when-applicable",
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
        "golang + gleam + elixir + erlang + installed build-output smoke + "
        "independent TypeSpec/JSON-Schema generation + bidirectional witnesses + "
        "four-way Diesel/SeaORM catalog and row-level convergence"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())