#!/usr/bin/env python3
"""Apply the final permanent DEN-3959 ORM admission wiring.

This temporary applicator is deleted by the proof workflow in the same commit
that lands the proven policy, documentation, and validation changes.
"""

from __future__ import annotations

import json
from pathlib import Path


def replace_once(path: Path, old: str, new: str, label: str) -> None:
    text = path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected one match, found {count}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def patch_check_zpkg() -> None:
    path = Path("scripts/check_zpkg.py")
    replace_once(
        path,
        '''EXPECTED_FLOWS = {
    "typespec": ["sql-when-applicable", "protobuf", "grpc", "wire-clients"],
    "json-schema-openapi": ["interfaces-types", "sql-when-applicable", "write-clients"],
}
''',
        '''EXPECTED_FLOWS = {
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
''',
        "authority flow expectations",
    )
    replace_once(
        path,
        '''    "persistence:check": "python3 scripts/orm_matrix_gate.py",
''',
        '''    "persistence:catalog": "python3 scripts/orm_matrix_gate.py",
    "persistence:data-plane": "python3 scripts/orm_data_plane_gate.py",
    "persistence:check": "npm run persistence:catalog && npm run persistence:data-plane",
''',
        "workspace persistence commands",
    )
    replace_once(
        path,
        '''        "scripts/orm_matrix_gate.py",
        "scripts/test_orm_matrix_gate.py",
        "scripts/orm_catalog_gate.py",
''',
        '''        "scripts/orm_matrix_gate.py",
        "scripts/test_orm_matrix_gate.py",
        "scripts/orm_data_plane_gate.py",
        "scripts/test_orm_data_plane_gate.py",
        "scripts/orm_catalog_gate.py",
        "docs/orm-data-plane-convergence.md",
''',
        "required ORM gate paths",
    )

    insertion_anchor = '''
    smoke_test = manifest.get("publish", {}).get("smoke_test", "")
'''
    insertion = '''
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
'''
    replace_once(path, insertion_anchor, insertion, "data-plane package validation")
    replace_once(
        path,
        '''        "diesel-seaorm-catalog-parity-when-applicable",
''',
        '''        "diesel-seaorm-catalog-parity-when-applicable",
        "diesel-seaorm-row-level-parity-when-applicable",
''',
        "row-level topology gate",
    )
    replace_once(
        path,
        '''        "four-way Diesel/SeaORM database-backed convergence"
''',
        '''        "four-way Diesel/SeaORM catalog and row-level convergence"
''',
        "validation success text",
    )


def patch_audit() -> None:
    path = Path("scripts/audit.py")
    replace_once(
        path,
        '''                    "The TypeSpec lane emits a compile-checked Diesel-shaped "
                    "witness and the JSON Schema/OpenAPI lane emits a "
                    "compile-checked SeaORM-shaped witness; normalized "
                    "persistence semantics are compared."
''',
        '''                    "TypeSpec and JSON Schema/OpenAPI each emit compile-checked "
                    "Diesel and SeaORM witnesses; all four manifests and "
                    "normalized persistence semantics are compared."
''',
        "audit ORM applicability",
    )
    replace_once(
        path,
        '''                    "Real Diesel and SeaORM compilation plus independent "
                    "PostgreSQL application and pg_catalog read-back remain "
                    "mandatory before persistence artifacts are admitted."
''',
        '''                    "Real Diesel and SeaORM compilation, independent SQL "
                    "application, pg_catalog read-back, and four-way row-level "
                    "insert/read/rejection convergence remain mandatory before "
                    "persistence artifacts are admitted."
''',
        "audit database gate applicability",
    )


def patch_architecture() -> None:
    path = Path("docs/architecture.md")
    replace_once(
        path,
        '''TypeSpec
  -> SQL_T where applicable
  -> Protobuf
  -> gRPC
  -> wire clients

JSON Schema/OpenAPI
  -> interfaces/types/runtime validators
  -> SQL_J where applicable
  -> write clients
''',
        '''TypeSpec
  -> interfaces/types/runtime code
  -> SQL_T where applicable
  -> Diesel_T and SeaORM_T
  -> Protobuf
  -> gRPC
  -> wire clients

JSON Schema/OpenAPI
  -> interfaces/types/runtime code
  -> SQL_J where applicable
  -> Diesel_J and SeaORM_J
  -> OpenAPI
  -> write clients
''',
        "peer authority flow diagram",
    )
    paragraph = '''CLI evidence paths are resolved against the selected repository root before
artifacts are hashed, so a valid custom `--output-root` cannot fail only during
receipt construction.
'''
    addition = paragraph + '''
`scripts/orm_data_plane_gate.py` then executes real insert and read-back paths
through all four authority/ORM combinations. It verifies optional-null behavior
and rejects duplicate primary keys, duplicate tenant/idempotency keys, invalid
enum values, missing required values, int32 overflow, and malformed timestamps.
Normalized positive rows and all rejection outcomes must agree across the full
matrix. See `docs/orm-data-plane-convergence.md` for the retained receipt and
promotion boundary.
'''
    replace_once(path, paragraph, addition, "architecture row-level gate")


def patch_zpkg_comment() -> None:
    path = Path(".zpkg.toml")
    replace_once(
        path,
        '''# the database-backed four-way ORM/catalog gate remains in service-enabled CI.
''',
        '''# the database-backed four-way ORM catalog and row-level gates remain in
# service-enabled CI.
''',
        "zed database gate comment",
    )


def patch_test_formatting() -> None:
    path = Path("scripts/test_orm_data_plane_gate.py")
    replace_once(
        path,
        '''        self.assertIn(
    'tinyvec = { version = "=1.13.0", default-features = false, features = ["std"] }',
    cargo,
)
''',
        '''        self.assertIn(
            'tinyvec = { version = "=1.13.0", default-features = false, features = ["std"] }',
            cargo,
        )
''',
        "data-plane test indentation",
    )


def verify_json_documents() -> None:
    json.loads(Path("package.json").read_text(encoding="utf-8"))
    json.loads(Path("contracts/authority-topology.json").read_text(encoding="utf-8"))


def main() -> None:
    patch_check_zpkg()
    patch_audit()
    patch_architecture()
    patch_zpkg_comment()
    patch_test_formatting()
    verify_json_documents()


if __name__ == "__main__":
    main()
