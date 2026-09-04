#!/usr/bin/env python3
"""Compatibility helpers for the full peer-authority ORM matrix.

TypeSpec and JSON Schema/OpenAPI remain independent top-level authorities. Each
source must generate SQL plus both Diesel and SeaORM code. This module retains
shared catalog and code-generation helpers for compatibility; its executable
entrypoint delegates to ``scripts.orm_matrix_gate`` so no command can admit the
old asymmetric TypeSpec-to-Diesel / JSON-Schema-to-SeaORM pairing.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.schema_convergence import (
    JSON_REL,
    TSP_REL,
    Model,
    canonical,
    parse_json_schema,
    parse_typespec,
    render_sql,
    run as run_schema_convergence,
)

DIESEL_VERSION = "2.3.12"
SEA_ORM_VERSION = "2.0.2"


@dataclass(frozen=True)
class Discrepancy:
    fingerprint: str
    kind: str
    detail: str
    resolutionState: str = "unexplained"


def now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def discrepancy(kind: str, detail: str) -> Discrepancy:
    digest = sha256_bytes(f"{kind}\0{detail}".encode())
    return Discrepancy(digest, kind, detail)


def write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def write_json(path: Path, value: Any) -> None:
    write_text(path, json.dumps(value, indent=2, sort_keys=True) + "\n")


def compare(
    label: str,
    left: Any,
    right: Any,
    kind: str,
    findings: list[Discrepancy],
) -> None:
    if left != right:
        findings.append(
            discrepancy(
                kind,
                f"{label}: left={json.dumps(left, sort_keys=True)}; "
                f"right={json.dumps(right, sort_keys=True)}",
            )
        )


def diesel_sql_type(field: Any) -> str:
    base = {
        "text": "Text",
        "integer": "Int4",
        "timestamptz": "Timestamptz",
    }[field.sql_type]
    return f"Nullable<{base}>" if field.nullable else base


def diesel_rust_type(field: Any) -> str:
    if field.logical_type == "int32":
        base = "i32"
    elif field.logical_type == "datetime":
        base = "chrono::DateTime<chrono::Utc>"
    else:
        base = "String"
    return f"Option<{base}>" if field.nullable else base


def seaorm_rust_type(field: Any) -> str:
    if field.logical_type == "int32":
        base = "i32"
    elif field.logical_type == "datetime":
        base = "DateTimeUtc"
    else:
        base = "String"
    return f"Option<{base}>" if field.nullable else base


def orm_manifest(
    model: Model,
    orm: str,
    authority: str,
    rust_type_fn: Any,
) -> dict[str, Any]:
    return {
        "schema": "ores.real-orm-manifest/v1",
        "orm": orm,
        "sourceAuthority": authority,
        "contract": canonical(model),
        "rustTypes": [
            {"column": field.column, "type": rust_type_fn(field)}
            for field in model.fields
        ],
    }


def render_cargo() -> str:
    return f'''[package]
name = "ores-middleware-orm-convergence-witness"
version = "0.0.0"
edition = "2024"
publish = false

[workspace]

[dependencies]
chrono = "0.4"
diesel = {{ version = "={DIESEL_VERSION}", default-features = false, features = ["postgres", "chrono"] }}
sea-orm = {{ version = "={SEA_ORM_VERSION}", default-features = false, features = ["macros", "with-chrono"] }}
serde_json = "1"
'''


def raw_json(value: Any) -> str:
    text = json.dumps(value, separators=(",", ":"), sort_keys=True)
    return 'r###"' + text + '"###'


def render_rust(typespec_model: Model, json_model: Model) -> str:
    diesel_fields = "\n".join(
        f"            {field.column} -> {diesel_sql_type(field)},"
        for field in typespec_model.fields
    )
    diesel_struct = "\n".join(
        f"        pub {field.column}: {diesel_rust_type(field)},"
        for field in typespec_model.fields
    )
    json_primary_keys = set(json_model.primary_key)
    seaorm_fields: list[str] = []
    for field in json_model.fields:
        attribute = (
            "        #[sea_orm(primary_key, auto_increment = false)]\n"
            if field.name in json_primary_keys
            else ""
        )
        seaorm_fields.append(
            attribute + f"        pub {field.column}: {seaorm_rust_type(field)},"
        )

    diesel_manifest = raw_json(
        orm_manifest(
            typespec_model,
            "diesel",
            "typespec",
            diesel_rust_type,
        )
    )
    seaorm_manifest = raw_json(
        orm_manifest(
            json_model,
            "seaorm",
            "json-schema-openapi",
            seaorm_rust_type,
        )
    )
    return f'''#![forbid(unsafe_code)]
#![allow(dead_code)]

mod diesel_lane {{
    diesel::table! {{
        {typespec_model.table} (id) {{
{diesel_fields}
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
    #[diesel(table_name = {typespec_model.table})]
    #[diesel(primary_key(id))]
    #[diesel(check_for_backend(diesel::pg::Pg))]
    pub struct IdempotencyRecord {{
{diesel_struct}
    }}

    pub fn manifest() -> serde_json::Value {{
        serde_json::from_str({diesel_manifest})
            .expect("generated Diesel manifest is valid JSON")
    }}
}}

mod seaorm_lane {{
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "{json_model.table}")]
    pub struct Model {{
{chr(10).join(seaorm_fields)}
    }}

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {{}}

    impl ActiveModelBehavior for ActiveModel {{}}

    pub fn manifest() -> serde_json::Value {{
        serde_json::from_str({seaorm_manifest})
            .expect("generated SeaORM manifest is valid JSON")
    }}
}}

fn main() {{
    let lane = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "both".to_owned());
    let value = match lane.as_str() {{
        "diesel" => diesel_lane::manifest(),
        "seaorm" => seaorm_lane::manifest(),
        "both" => serde_json::json!({{
            "diesel": diesel_lane::manifest(),
            "seaorm": seaorm_lane::manifest(),
        }}),
        other => panic!("unknown ORM lane: {{other}}"),
    }};
    println!(
        "{{}}",
        serde_json::to_string(&value).expect("manifest serializes")
    );
}}
'''


def run_command(
    command: list[str],
    cwd: Path,
    *,
    env: dict[str, str] | None = None,
    timeout: int = 1200,
) -> str:
    merged = os.environ.copy()
    if env:
        merged.update(env)
    completed = subprocess.run(
        command,
        cwd=cwd,
        env=merged,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
        timeout=timeout,
    )
    if completed.returncode:
        raise ValueError(
            f"command failed ({' '.join(command)}): {completed.stdout.strip()}"
        )
    return completed.stdout.strip()


def tool_version(command: list[str]) -> str:
    try:
        value = run_command(command, ROOT, timeout=30)
    except (OSError, subprocess.SubprocessError, ValueError):
        return "unavailable"
    return value.splitlines()[0] if value else "unavailable"


def build_real_orm_witnesses(
    root: Path,
    output_root: Path,
    typespec_model: Model,
    json_model: Model,
) -> tuple[dict[str, Any], dict[str, str], Path]:
    crate = output_root / "rust-witness"
    write_text(crate / "Cargo.toml", render_cargo())
    write_text(crate / "src/main.rs", render_rust(typespec_model, json_model))
    cargo = shutil.which("cargo")
    if not cargo:
        raise ValueError("cargo is required for the real Diesel/SeaORM gate")
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
    for lane in ("diesel", "seaorm"):
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


def redact_database_url(database_url: str) -> dict[str, Any]:
    parsed = urlsplit(database_url)
    return {
        "scheme": parsed.scheme,
        "host": parsed.hostname,
        "port": parsed.port,
        "database": parsed.path.lstrip("/"),
    }


def psql(database_url: str, sql: str) -> str:
    executable = shutil.which("psql")
    if not executable:
        raise ValueError("psql is required for the database-backed catalog gate")
    completed = subprocess.run(
        [
            executable,
            database_url,
            "--no-psqlrc",
            "--set",
            "ON_ERROR_STOP=1",
            "--quiet",
            "--tuples-only",
            "--no-align",
        ],
        input=sql,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
        timeout=180,
    )
    if completed.returncode:
        raise ValueError(f"psql failed: {completed.stdout.strip()}")
    return completed.stdout.strip()


def json_rows(database_url: str, query: str) -> list[dict[str, Any]]:
    output = psql(database_url, query)
    return [json.loads(line) for line in output.splitlines() if line.strip()]


def sql_string(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def normalize_index_definition(value: str, schema: str) -> str:
    return re.sub(rf"\bON\s+{re.escape(schema)}\.", "ON ", value)


def read_catalog(
    database_url: str,
    schema: str,
    table: str,
) -> dict[str, Any]:
    columns = json_rows(
        database_url,
        f"""
SELECT json_build_object(
  'ordinalPosition', ordinal_position,
  'column', column_name,
  'dataType', data_type,
  'udtName', udt_name,
  'nullable', is_nullable = 'YES',
  'default', column_default
)::text
FROM information_schema.columns
WHERE table_schema = {sql_string(schema)}
  AND table_name = {sql_string(table)}
ORDER BY ordinal_position;
""",
    )
    constraints = json_rows(
        database_url,
        f"""
SELECT json_build_object(
  'name', c.conname,
  'type', c.contype,
  'definition', pg_get_constraintdef(c.oid, true)
)::text
FROM pg_constraint c
JOIN pg_class t ON t.oid = c.conrelid
JOIN pg_namespace n ON n.oid = t.relnamespace
WHERE n.nspname = {sql_string(schema)}
  AND t.relname = {sql_string(table)}
ORDER BY c.contype, c.conname;
""",
    )
    indexes = json_rows(
        database_url,
        f"""
SELECT json_build_object(
  'name', indexname,
  'definition', indexdef
)::text
FROM pg_indexes
WHERE schemaname = {sql_string(schema)}
  AND tablename = {sql_string(table)}
ORDER BY indexname;
""",
    )
    for item in indexes:
        item["definition"] = normalize_index_definition(item["definition"], schema)
    return {
        "table": table,
        "columns": columns,
        "constraints": constraints,
        "indexes": indexes,
    }


def expected_columns(model: Model) -> list[dict[str, Any]]:
    sql_types = {
        "text": ("text", "text"),
        "integer": ("integer", "int4"),
        "timestamptz": ("timestamp with time zone", "timestamptz"),
    }
    return [
        {
            "ordinalPosition": index + 1,
            "column": field.column,
            "dataType": sql_types[field.sql_type][0],
            "udtName": sql_types[field.sql_type][1],
            "nullable": field.nullable,
            "default": None,
        }
        for index, field in enumerate(model.fields)
    ]


def validate_catalog(
    model: Model,
    catalog: dict[str, Any],
    lane: str,
    findings: list[Discrepancy],
) -> None:
    compare(
        f"{lane} catalog columns",
        expected_columns(model),
        catalog["columns"],
        f"{lane}-catalog-column-mismatch",
        findings,
    )
    names = {field.name: field.column for field in model.fields}
    expected_constraints: set[tuple[str, str]] = {
        (f"pk_{model.table}", "p")
    }
    for group in model.unique:
        suffix = "_".join(names[name] for name in group)
        expected_constraints.add((f"uq_{model.table}_{suffix}", "u"))
    for field in model.fields:
        if field.enum_values:
            expected_constraints.add((f"ck_{model.table}_{field.column}", "c"))
    actual_constraints = {
        (item["name"], item["type"]) for item in catalog["constraints"]
    }
    compare(
        f"{lane} catalog constraint identities",
        sorted(expected_constraints),
        sorted(actual_constraints),
        f"{lane}-catalog-constraint-mismatch",
        findings,
    )

    definitions = {
        item["name"]: item["definition"] for item in catalog["constraints"]
    }
    for field in model.fields:
        if not field.enum_values:
            continue
        definition = definitions.get(f"ck_{model.table}_{field.column}", "")
        missing = [value for value in field.enum_values if value not in definition]
        if missing:
            findings.append(
                discrepancy(
                    f"{lane}-catalog-enum-check-mismatch",
                    f"{lane} enum check for {field.column} misses {missing}: "
                    f"{definition}",
                )
            )

    expected_indexes = {f"pk_{model.table}"}
    for group in model.unique:
        suffix = "_".join(names[name] for name in group)
        expected_indexes.add(f"uq_{model.table}_{suffix}")
    actual_indexes = {item["name"] for item in catalog["indexes"]}
    compare(
        f"{lane} catalog index identities",
        sorted(expected_indexes),
        sorted(actual_indexes),
        f"{lane}-catalog-index-mismatch",
        findings,
    )


ALLOWED_CATALOG_SCHEMAS = frozenset(
    {
        "typespec_lane",
        "json_schema_lane",
        "typespec_data_plane",
        "json_schema_data_plane",
    }
)


def apply_lane(database_url: str, schema: str, sql: str) -> None:
    if schema not in ALLOWED_CATALOG_SCHEMAS:
        raise ValueError(f"unsupported catalog schema {schema!r}")
    psql(
        database_url,
        f"DROP SCHEMA IF EXISTS {schema} CASCADE;\n"
        f"CREATE SCHEMA {schema};\n"
        f"SET search_path TO {schema}, public;\n"
        f"{sql}",
    )


def source_digests(root: Path) -> dict[str, str]:
    return {
        TSP_REL.as_posix(): sha256_bytes((root / TSP_REL).read_bytes()),
        JSON_REL.as_posix(): sha256_bytes((root / JSON_REL).read_bytes()),
    }


def run(
    root: Path = ROOT,
    output_root: Path | None = None,
    database_url: str | None = None,
) -> tuple[list[Discrepancy], dict[str, Any]]:
    started = now()
    output_root = output_root or root / "target" / "orm-catalog-gate"
    database_url = database_url or os.environ.get("DATABASE_URL")
    if not database_url:
        raise ValueError("DATABASE_URL or --database-url is required")

    projection_findings, projection_report = run_schema_convergence(
        root,
        root / "target" / "schema-convergence",
    )
    findings = [
        discrepancy(item.kind, item.detail) for item in projection_findings
    ]
    typespec_model = parse_typespec(root)
    json_model = parse_json_schema(root)

    manifests, versions, crate = build_real_orm_witnesses(
        root,
        output_root,
        typespec_model,
        json_model,
    )
    expected_diesel = orm_manifest(
        typespec_model,
        "diesel",
        "typespec",
        diesel_rust_type,
    )
    expected_seaorm = orm_manifest(
        json_model,
        "seaorm",
        "json-schema-openapi",
        seaorm_rust_type,
    )
    compare(
        "compiled Diesel manifest",
        expected_diesel,
        manifests["diesel"],
        "diesel-compiled-manifest-mismatch",
        findings,
    )
    compare(
        "compiled SeaORM manifest",
        expected_seaorm,
        manifests["seaorm"],
        "seaorm-compiled-manifest-mismatch",
        findings,
    )
    compare(
        "Diesel/SeaORM contract semantics",
        manifests["diesel"]["contract"],
        manifests["seaorm"]["contract"],
        "diesel-seaorm-real-model-mismatch",
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
    ]
    artifacts = {
        path.relative_to(root).as_posix(): sha256_bytes(path.read_bytes())
        for path in artifact_paths
    }
    report = {
        "schema": "ores.orm-catalog-convergence-report/v1",
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
            "typespec": ["SQL_T", "Diesel"],
            "json-schema-openapi": ["SQL_J", "SeaORM"],
        },
        "tools": {
            "python": sys.version.split()[0],
            "cargo": tool_version(["cargo", "--version"]),
            "rustc": tool_version(["rustc", "--version"]),
            "psql": tool_version(["psql", "--version"]),
            "diesel": versions.get("diesel", "unresolved"),
            "sea-orm": versions.get("sea-orm", "unresolved"),
        },
        "postgres": postgres,
        "projectionReport": projection_report,
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


def legacy_main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--output-root", type=Path)
    parser.add_argument("--database-url")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    output_root = args.output_root or root / "target" / "orm-catalog-gate"
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
            "orm-catalog-gate-failure",
            f"{type(exc).__name__}: {exc}",
        )
        findings = [item]
        report = {
            "schema": "ores.orm-catalog-convergence-report/v1",
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
            f"STOPPED_FOR_EVALUATION: {len(findings)} ORM/catalog "
            f"discrepancy(s); report={report_path}"
        )
        for item in findings:
            print(f"- {item.fingerprint}: {item.kind}: {item.detail}")
        return 2
    print(
        "real Diesel/SeaORM and PostgreSQL catalog convergence passed; "
        f"report={report_path}"
    )
    return 0


def main() -> int:
    """Run the only supported four-way ORM and PostgreSQL admission gate."""
    from scripts.orm_matrix_gate import main as matrix_main

    return matrix_main()


if __name__ == "__main__":
    raise SystemExit(main())
