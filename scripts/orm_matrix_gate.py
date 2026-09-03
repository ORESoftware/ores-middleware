#!/usr/bin/env python3
"""Compile the full TypeSpec/JSON-Schema x Diesel/SeaORM witness matrix.

This is the database-backed companion to the Rust polyglot generator. TypeSpec
and JSON Schema/OpenAPI remain independent top-level authorities. Each lane
independently produces SQL plus both Diesel and SeaORM witnesses. Every source,
ORM, SQL, compiled-manifest, and PostgreSQL-catalog discrepancy stops promotion.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from dataclasses import asdict
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.orm_catalog_gate import (
    Discrepancy,
    apply_lane,
    compare,
    diesel_rust_type,
    diesel_sql_type,
    discrepancy,
    now,
    orm_manifest,
    parse_json_schema,
    parse_typespec,
    psql,
    read_catalog,
    redact_database_url,
    render_cargo,
    render_sql,
    run_command,
    seaorm_rust_type,
    sha256_bytes,
    source_digests,
    tool_version,
    validate_catalog,
    write_json,
    write_text,
)
from scripts.schema_convergence import Model

LANES = (
    "typespec-diesel",
    "typespec-seaorm",
    "json-schema-openapi-diesel",
    "json-schema-openapi-seaorm",
)


def raw_json(value: Any) -> str:
    text = json.dumps(value, separators=(",", ":"), sort_keys=True)
    return 'r###"' + text + '"###'


def diesel_module(module_name: str, model: Model, authority: str) -> str:
    fields = "\n".join(
        f"            {field.column} -> {diesel_sql_type(field)},"
        for field in model.fields
    )
    struct_fields = "\n".join(
        f"        pub {field.column}: {diesel_rust_type(field)},"
        for field in model.fields
    )
    manifest = raw_json(
        orm_manifest(model, "diesel", authority, diesel_rust_type)
    )
    primary_key = ", ".join(
        next(field.column for field in model.fields if field.name == name)
        for name in model.primary_key
    )
    return f'''mod {module_name} {{
    diesel::table! {{
        {model.table} ({primary_key}) {{
{fields}
        }}
    }}

    #[derive(
        Clone,
        Debug,
        diesel::Queryable,
        diesel::Selectable,
        diesel::Insertable,
        diesel::Identifiable,
    )]
    #[diesel(table_name = {model.table})]
    #[diesel(primary_key({primary_key}))]
    #[diesel(check_for_backend(diesel::pg::Pg))]
    pub struct IdempotencyRecord {{
{struct_fields}
    }}

    pub fn manifest() -> serde_json::Value {{
        serde_json::from_str({manifest})
            .expect("generated Diesel manifest is valid JSON")
    }}
}}
'''


def seaorm_module(module_name: str, model: Model, authority: str) -> str:
    primary_keys = set(model.primary_key)
    fields: list[str] = []
    for field in model.fields:
        attribute = (
            "        #[sea_orm(primary_key, auto_increment = false)]\n"
            if field.name in primary_keys
            else ""
        )
        fields.append(
            attribute + f"        pub {field.column}: {seaorm_rust_type(field)},"
        )
    manifest = raw_json(
        orm_manifest(model, "seaorm", authority, seaorm_rust_type)
    )
    return f'''mod {module_name} {{
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "{model.table}")]
    pub struct Model {{
{chr(10).join(fields)}
    }}

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {{}}

    impl ActiveModelBehavior for ActiveModel {{}}

    pub fn manifest() -> serde_json::Value {{
        serde_json::from_str({manifest})
            .expect("generated SeaORM manifest is valid JSON")
    }}
}}
'''


def render_rust_matrix(typespec_model: Model, json_model: Model) -> str:
    modules = "\n".join(
        [
            diesel_module("typespec_diesel", typespec_model, "typespec"),
            seaorm_module("typespec_seaorm", typespec_model, "typespec"),
            diesel_module(
                "json_schema_openapi_diesel",
                json_model,
                "json-schema-openapi",
            ),
            seaorm_module(
                "json_schema_openapi_seaorm",
                json_model,
                "json-schema-openapi",
            ),
        ]
    )
    arms = "\n".join(
        [
            '        "typespec-diesel" => typespec_diesel::manifest(),',
            '        "typespec-seaorm" => typespec_seaorm::manifest(),',
            '        "json-schema-openapi-diesel" => json_schema_openapi_diesel::manifest(),',
            '        "json-schema-openapi-seaorm" => json_schema_openapi_seaorm::manifest(),',
        ]
    )
    return f'''#![forbid(unsafe_code)]
#![allow(dead_code)]

{modules}

fn main() {{
    let lane = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "all".to_owned());
    let value = match lane.as_str() {{
{arms}
        "all" => serde_json::json!({{
            "typespec-diesel": typespec_diesel::manifest(),
            "typespec-seaorm": typespec_seaorm::manifest(),
            "json-schema-openapi-diesel": json_schema_openapi_diesel::manifest(),
            "json-schema-openapi-seaorm": json_schema_openapi_seaorm::manifest(),
        }}),
        other => panic!("unknown ORM lane: {{other}}"),
    }};
    println!(
        "{{}}",
        serde_json::to_string(&value).expect("manifest serializes")
    );
}}
'''


def build_real_orm_matrix(
    root: Path,
    output_root: Path,
    typespec_model: Model,
    json_model: Model,
) -> tuple[dict[str, Any], dict[str, str], Path]:
    crate = output_root / "rust-witness"
    write_text(crate / "Cargo.toml", render_cargo())
    write_text(
        crate / "src/main.rs",
        render_rust_matrix(typespec_model, json_model),
    )
    cargo = shutil.which("cargo")
    if not cargo:
        raise ValueError("cargo is required for the four-way ORM gate")
    env = {"CARGO_TARGET_DIR": str(output_root / "cargo-target")}
    manifest_path = str(crate / "Cargo.toml")
    run_command(
        [cargo, "generate-lockfile", "--manifest-path", manifest_path],
        root,
        env=env,
    )
    run_command(
        [
            cargo,
            "check",
            "--locked",
            "--all-targets",
            "--manifest-path",
            manifest_path,
        ],
        root,
        env=env,
    )
    manifests: dict[str, Any] = {}
    for lane in LANES:
        output = run_command(
            [
                cargo,
                "run",
                "--quiet",
                "--locked",
                "--manifest-path",
                manifest_path,
                "--",
                lane,
            ],
            root,
            env=env,
        )
        manifests[lane] = json.loads(output)
    metadata = json.loads(
        run_command(
            [
                cargo,
                "metadata",
                "--locked",
                "--format-version",
                "1",
                "--manifest-path",
                manifest_path,
            ],
            root,
            env=env,
        )
    )
    versions = {
        package["name"]: package["version"]
        for package in metadata["packages"]
        if package["name"] in {"diesel", "sea-orm"}
    }
    return manifests, versions, crate


def expected_manifests(
    typespec_model: Model,
    json_model: Model,
) -> dict[str, dict[str, Any]]:
    return {
        "typespec-diesel": orm_manifest(
            typespec_model,
            "diesel",
            "typespec",
            diesel_rust_type,
        ),
        "typespec-seaorm": orm_manifest(
            typespec_model,
            "seaorm",
            "typespec",
            seaorm_rust_type,
        ),
        "json-schema-openapi-diesel": orm_manifest(
            json_model,
            "diesel",
            "json-schema-openapi",
            diesel_rust_type,
        ),
        "json-schema-openapi-seaorm": orm_manifest(
            json_model,
            "seaorm",
            "json-schema-openapi",
            seaorm_rust_type,
        ),
    }


def compare_orm_matrix(
    manifests: dict[str, Any],
    expected: dict[str, dict[str, Any]],
    findings: list[Discrepancy],
) -> None:
    for lane in LANES:
        compare(
            f"compiled {lane} manifest",
            expected[lane],
            manifests[lane],
            f"{lane}-compiled-manifest-mismatch",
            findings,
        )

    contracts = {lane: manifests[lane]["contract"] for lane in LANES}
    baseline = contracts[LANES[0]]
    for lane in LANES[1:]:
        compare(
            f"four-way ORM contract semantics ({LANES[0]} vs {lane})",
            baseline,
            contracts[lane],
            "four-way-orm-contract-mismatch",
            findings,
        )

    compare(
        "Diesel mapping across source authorities",
        manifests["typespec-diesel"]["rustTypes"],
        manifests["json-schema-openapi-diesel"]["rustTypes"],
        "diesel-cross-authority-type-mismatch",
        findings,
    )
    compare(
        "SeaORM mapping across source authorities",
        manifests["typespec-seaorm"]["rustTypes"],
        manifests["json-schema-openapi-seaorm"]["rustTypes"],
        "seaorm-cross-authority-type-mismatch",
        findings,
    )


def run(
    root: Path = ROOT,
    output_root: Path | None = None,
    database_url: str | None = None,
) -> tuple[list[Discrepancy], dict[str, Any]]:
    started = now()
    output_root = output_root or root / "target" / "orm-matrix-gate"
    database_url = database_url or os.environ.get("DATABASE_URL")
    if not database_url:
        raise ValueError("DATABASE_URL or --database-url is required")

    polyglot_report = root / "target/schema-convergence/receipt.json"
    generator = shutil.which("cargo")
    if not generator:
        raise ValueError("cargo is required for the Rust polyglot generator")
    run_command(
        [
            generator,
            "run",
            "--quiet",
            "--manifest-path",
            str(root / "tools/contract-parity/Cargo.toml"),
            "--bin",
            "persistence_codegen",
            "--",
            "--root",
            str(root),
            "--output-root",
            "target/schema-convergence",
            "--report",
            "target/schema-convergence/receipt.json",
        ],
        root,
    )
    projection = json.loads(polyglot_report.read_text(encoding="utf-8"))
    findings: list[Discrepancy] = []
    if projection.get("status") != "passed" or not projection.get(
        "zeroUnexplainedFindings"
    ):
        findings.append(
            discrepancy(
                "polyglot-projection-gate-not-passed",
                f"Rust generator report status={projection.get('status')!r}",
            )
        )

    typespec_model = parse_typespec(root)
    json_model = parse_json_schema(root)
    manifests, versions, crate = build_real_orm_matrix(
        root,
        output_root,
        typespec_model,
        json_model,
    )
    compare_orm_matrix(
        manifests,
        expected_manifests(typespec_model, json_model),
        findings,
    )

    sql_typespec = render_sql(typespec_model)
    sql_json = render_sql(json_model)
    typespec_sql_path = output_root / "sql/typespec.sql"
    json_sql_path = output_root / "sql/json-schema-openapi.sql"
    write_text(typespec_sql_path, sql_typespec)
    write_text(json_sql_path, sql_json)
    apply_lane(database_url, "typespec_lane", sql_typespec)
    apply_lane(database_url, "json_schema_lane", sql_json)
    catalog_typespec = read_catalog(
        database_url,
        "typespec_lane",
        typespec_model.table,
    )
    catalog_json = read_catalog(
        database_url,
        "json_schema_lane",
        json_model.table,
    )
    typespec_catalog_path = output_root / "catalog/typespec.json"
    json_catalog_path = output_root / "catalog/json-schema-openapi.json"
    write_json(typespec_catalog_path, catalog_typespec)
    write_json(json_catalog_path, catalog_json)
    validate_catalog(typespec_model, catalog_typespec, "typespec", findings)
    validate_catalog(json_model, catalog_json, "json-schema-openapi", findings)
    compare(
        "SQL_T/SQL_J normalized catalog read-back",
        catalog_typespec,
        catalog_json,
        "postgres-catalog-lane-mismatch",
        findings,
    )

    postgres = json.loads(
        psql(
            database_url,
            "SELECT json_build_object("
            "'serverVersion', version(), "
            "'serverVersionNum', current_setting('server_version_num')"
            ")::text;",
        )
    )
    artifact_paths = [
        crate / "Cargo.toml",
        crate / "Cargo.lock",
        crate / "src/main.rs",
        typespec_sql_path,
        json_sql_path,
        typespec_catalog_path,
        json_catalog_path,
        polyglot_report,
    ]
    artifacts = {
        path.relative_to(root).as_posix(): sha256_bytes(path.read_bytes())
        for path in artifact_paths
    }
    report = {
        "schema": "ores.orm-catalog-convergence-report/v2",
        "startedAt": started,
        "endedAt": now(),
        "actor": os.environ.get("GITHUB_ACTOR")
        or os.environ.get("USER")
        or "unknown",
        "scope": {
            "commit": os.environ.get("GITHUB_SHA"),
            "sourceDigests": source_digests(root),
            "database": redact_database_url(database_url),
            "schemas": ["typespec_lane", "json_schema_lane"],
        },
        "authorities": ["typespec", "json-schema-openapi"],
        "lanes": {
            "typespec": ["SQL_T", "Diesel_T", "SeaORM_T"],
            "json-schema-openapi": ["SQL_J", "Diesel_J", "SeaORM_J"],
        },
        "crossChecks": [
            "TypeSpec Diesel vs TypeSpec SeaORM",
            "JSON Schema Diesel vs JSON Schema SeaORM",
            "TypeSpec Diesel vs JSON Schema Diesel",
            "TypeSpec SeaORM vs JSON Schema SeaORM",
            "SQL_T vs SQL_J PostgreSQL catalog read-back",
        ],
        "tools": {
            "python": sys.version.split()[0],
            "cargo": tool_version(["cargo", "--version"]),
            "rustc": tool_version(["rustc", "--version"]),
            "psql": tool_version(["psql", "--version"]),
            "diesel": versions.get("diesel", "unresolved"),
            "sea-orm": versions.get("sea-orm", "unresolved"),
        },
        "postgres": postgres,
        "polyglotProjectionReport": projection,
        "ormManifests": manifests,
        "catalogs": {
            "typespec": catalog_typespec,
            "json-schema-openapi": catalog_json,
        },
        "artifacts": artifacts,
        "status": "stopped_for_evaluation" if findings else "passed",
        "zeroUnexplainedFindings": not findings,
        "discrepancies": [asdict(item) for item in findings],
    }
    return findings, report


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--output-root", type=Path)
    parser.add_argument("--database-url")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    output_root = args.output_root or root / "target" / "orm-matrix-gate"
    started = now()
    try:
        findings, report = run(root, output_root, args.database_url)
    except (
        OSError,
        KeyError,
        TypeError,
        ValueError,
        json.JSONDecodeError,
        subprocess.SubprocessError,
    ) as exc:
        item = discrepancy(
            "orm-matrix-gate-failure",
            f"{type(exc).__name__}: {exc}",
        )
        findings = [item]
        report = {
            "schema": "ores.orm-catalog-convergence-report/v2",
            "startedAt": started,
            "endedAt": now(),
            "actor": os.environ.get("GITHUB_ACTOR")
            or os.environ.get("USER")
            or "unknown",
            "status": "stopped_for_evaluation",
            "zeroUnexplainedFindings": False,
            "discrepancies": [asdict(item)],
        }
    report_path = args.report or output_root / "receipt.json"
    write_json(report_path, report)
    if findings:
        print(
            f"STOPPED_FOR_EVALUATION: {len(findings)} four-way ORM/catalog "
            f"discrepancy(s); report={report_path}"
        )
        for item in findings:
            print(f"- {item.fingerprint}: {item.kind}: {item.detail}")
        return 2
    print(
        "four-way TypeSpec/JSON-Schema x Diesel/SeaORM and PostgreSQL "
        f"catalog convergence passed; report={report_path}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
