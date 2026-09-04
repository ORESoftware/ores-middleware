#!/usr/bin/env python3
"""Execute TypeSpec/JSON-Schema x Diesel/SeaORM row-level convergence.

Both authored authorities independently generate SQL and ORM models. This gate
applies each SQL lane to an isolated PostgreSQL schema, then executes insert,
read-back, optional-null, primary-key, unique-key, enum/check, not-null, int32,
and timestamp cases through real Diesel and SeaORM connections. No authority or
ORM is preferred; every unexplained difference stops evaluation.
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
    DIESEL_VERSION,
    SEA_ORM_VERSION,
    Discrepancy,
    apply_lane,
    compare,
    diesel_rust_type,
    diesel_sql_type,
    discrepancy,
    now,
    parse_json_schema,
    parse_typespec,
    redact_database_url,
    render_sql,
    seaorm_rust_type,
    sha256_bytes,
    source_digests,
    tool_version,
    write_json,
    write_text,
)
from scripts.schema_convergence import Model
from scripts.subprocess_capture import run_command

DATA_PLANE_LANES: dict[str, tuple[str, str, str]] = {
    "typespec-diesel": ("typespec", "diesel", "typespec_data_plane"),
    "typespec-seaorm": ("typespec", "seaorm", "typespec_data_plane"),
    "json-schema-openapi-diesel": (
        "json-schema-openapi",
        "diesel",
        "json_schema_data_plane",
    ),
    "json-schema-openapi-seaorm": (
        "json-schema-openapi",
        "seaorm",
        "json_schema_data_plane",
    ),
}

EXPECTED_FIELDS = (
    ("id", "id", "string", False),
    ("tenantId", "tenant_id", "string", False),
    ("idempotencyKey", "idempotency_key", "string", False),
    ("requestHash", "request_hash", "string", False),
    ("status", "status", "enum", False),
    ("responseStatus", "response_status", "int32", True),
    ("responseBody", "response_body", "string", True),
    ("createdAt", "created_at", "datetime", False),
    ("expiresAt", "expires_at", "datetime", False),
)
EXPECTED_REJECTIONS = (
    "primaryKey",
    "uniqueKey",
    "enumCheck",
    "requiredNotNull",
    "int32Overflow",
    "invalidTimestamp",
)


def resolve_cli_path(root: Path, value: Path | None, default: Path) -> Path:
    candidate = default if value is None else value
    if not candidate.is_absolute():
        candidate = root / candidate
    return candidate.resolve()


def validate_supported_model(model: Model, authority: str) -> None:
    actual = tuple(
        (field.name, field.column, field.logical_type, field.nullable)
        for field in model.fields
    )
    if actual != EXPECTED_FIELDS:
        raise ValueError(
            f"{authority} data-plane model is unsupported: "
            f"expected={EXPECTED_FIELDS!r}; actual={actual!r}"
        )
    if model.table != "middleware_idempotency":
        raise ValueError(
            f"{authority} data-plane table must be middleware_idempotency, "
            f"got {model.table!r}"
        )
    if tuple(model.primary_key) != ("id",):
        raise ValueError(
            f"{authority} data-plane primary key must be ('id',), "
            f"got {tuple(model.primary_key)!r}"
        )
    if tuple(tuple(group) for group in model.unique) != (
        ("tenantId", "idempotencyKey"),
    ):
        raise ValueError(
            f"{authority} data-plane unique key mismatch: {model.unique!r}"
        )
    statuses = next(
        field.enum_values
        for field in model.fields
        if field.name == "status"
    )
    if tuple(statuses) != ("pending", "succeeded", "failed"):
        raise ValueError(
            f"{authority} status vocabulary mismatch: {statuses!r}"
        )


def render_data_plane_cargo() -> str:
    return f'''[package]
name = "ores-middleware-orm-data-plane-witness"
version = "0.0.0"
edition = "2024"
publish = false

[workspace]

[dependencies]
chrono = "0.4"
diesel = {{ version = "={DIESEL_VERSION}", default-features = false, features = ["postgres", "chrono"] }}
sea-orm = {{ version = "={SEA_ORM_VERSION}", default-features = false, features = ["macros", "with-chrono", "sqlx-postgres", "runtime-tokio-rustls"] }}
# SQLx's PostgreSQL stringprep path enables tinyvec's alloc surface.
# Explicit std feature unification keeps tinyvec 1.13.0 compilable on
# the pinned Rust toolchain without broadening database backends.
tinyvec = {{ version = "=1.13.0", default-features = false, features = ["std"] }}
serde_json = "1"
tokio = {{ version = "1", features = ["macros", "rt-multi-thread"] }}
'''


def diesel_data_plane_module(
    module_name: str,
    model: Model,
    authority: str,
    schema: str,
) -> str:
    fields = "\n".join(
        f"            {field.column} -> {diesel_sql_type(field)},"
        for field in model.fields
    )
    struct_fields = "\n".join(
        f"        pub {field.column}: {diesel_rust_type(field)},"
        for field in model.fields
    )
    template = r'''mod __MODULE__ {
    use chrono::{DateTime, SecondsFormat, Utc};
    use diesel::connection::SimpleConnection;
    use diesel::pg::PgConnection;
    use diesel::prelude::*;

    diesel::table! {
        __TABLE__ (id) {
__FIELDS__
        }
    }

    #[derive(
        Clone,
        Debug,
        diesel::Queryable,
        diesel::Selectable,
        diesel::Insertable,
        diesel::Identifiable,
    )]
    #[diesel(table_name = __TABLE__)]
    #[diesel(primary_key(id))]
    #[diesel(check_for_backend(diesel::pg::Pg))]
    pub struct IdempotencyRecord {
__STRUCT_FIELDS__
    }

    fn instant(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("checked witness timestamp")
            .with_timezone(&Utc)
    }

    fn record(
        id: &str,
        key: &str,
        status: &str,
        response_status: Option<i32>,
        response_body: Option<&str>,
    ) -> IdempotencyRecord {
        IdempotencyRecord {
            id: id.to_owned(),
            tenant_id: "tenant-a".to_owned(),
            idempotency_key: key.to_owned(),
            request_hash: format!("hash-{id}"),
            status: status.to_owned(),
            response_status,
            response_body: response_body.map(str::to_owned),
            created_at: instant("2026-09-03T20:00:00Z"),
            expires_at: instant("2026-09-04T20:00:00Z"),
        }
    }

    fn normalize(value: &IdempotencyRecord) -> serde_json::Value {
        serde_json::json!({
            "id": &value.id,
            "tenantId": &value.tenant_id,
            "idempotencyKey": &value.idempotency_key,
            "requestHash": &value.request_hash,
            "status": &value.status,
            "responseStatus": value.response_status,
            "responseBody": value.response_body.as_deref(),
            "createdAt": value.created_at.to_rfc3339_opts(SecondsFormat::Secs, true),
            "expiresAt": value.expires_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        })
    }

    pub fn execute(database_url: &str) -> Result<serde_json::Value, String> {
        let mut connection = PgConnection::establish(database_url)
            .map_err(|error| format!("Diesel connect failed: {error}"))?;
        connection
            .batch_execute("SET search_path TO __SCHEMA__, public")
            .map_err(|error| format!("Diesel search_path failed: {error}"))?;
        diesel::delete(__TABLE__::table)
            .execute(&mut connection)
            .map_err(|error| format!("Diesel cleanup failed: {error}"))?;

        let full = record("row-full", "key-full", "succeeded", Some(201), Some("ok"));
        diesel::insert_into(__TABLE__::table)
            .values(&full)
            .execute(&mut connection)
            .map_err(|error| format!("Diesel full insert failed: {error}"))?;
        let full_read: IdempotencyRecord = __TABLE__::table
            .find("row-full")
            .select(IdempotencyRecord::as_select())
            .first(&mut connection)
            .map_err(|error| format!("Diesel full read failed: {error}"))?;

        let optional_null = record("row-null", "key-null", "pending", None, None);
        diesel::insert_into(__TABLE__::table)
            .values(&optional_null)
            .execute(&mut connection)
            .map_err(|error| format!("Diesel optional-null insert failed: {error}"))?;
        let null_read: IdempotencyRecord = __TABLE__::table
            .find("row-null")
            .select(IdempotencyRecord::as_select())
            .first(&mut connection)
            .map_err(|error| format!("Diesel optional-null read failed: {error}"))?;

        let duplicate_primary = record(
            "row-full",
            "key-primary-duplicate",
            "pending",
            None,
            None,
        );
        let primary_key = diesel::insert_into(__TABLE__::table)
            .values(&duplicate_primary)
            .execute(&mut connection)
            .is_err();

        let duplicate_unique = record(
            "row-unique-duplicate",
            "key-full",
            "pending",
            None,
            None,
        );
        let unique_key = diesel::insert_into(__TABLE__::table)
            .values(&duplicate_unique)
            .execute(&mut connection)
            .is_err();

        let invalid_enum = record(
            "row-invalid-enum",
            "key-invalid-enum",
            "unknown",
            None,
            None,
        );
        let enum_check = diesel::insert_into(__TABLE__::table)
            .values(&invalid_enum)
            .execute(&mut connection)
            .is_err();

        let required_not_null = diesel::sql_query(
            "INSERT INTO __TABLE__ \
             (id, tenant_id, idempotency_key, request_hash, status, response_status, \
              response_body, created_at, expires_at) VALUES \
             ('row-null-required', NULL, 'key-null-required', 'hash-null-required', \
              'pending', NULL, NULL, TIMESTAMPTZ '2026-09-03T20:00:00Z', \
              TIMESTAMPTZ '2026-09-04T20:00:00Z')",
        )
        .execute(&mut connection)
        .is_err();

        let int32_overflow = diesel::sql_query(
            "INSERT INTO __TABLE__ \
             (id, tenant_id, idempotency_key, request_hash, status, response_status, \
              response_body, created_at, expires_at) VALUES \
             ('row-int32-overflow', 'tenant-a', 'key-int32-overflow', \
              'hash-int32-overflow', 'pending', 2147483648, NULL, \
              TIMESTAMPTZ '2026-09-03T20:00:00Z', \
              TIMESTAMPTZ '2026-09-04T20:00:00Z')",
        )
        .execute(&mut connection)
        .is_err();

        let invalid_timestamp = diesel::sql_query(
            "INSERT INTO __TABLE__ \
             (id, tenant_id, idempotency_key, request_hash, status, response_status, \
              response_body, created_at, expires_at) VALUES \
             ('row-invalid-time', 'tenant-a', 'key-invalid-time', \
              'hash-invalid-time', 'pending', NULL, NULL, 'not-a-time', \
              TIMESTAMPTZ '2026-09-04T20:00:00Z')",
        )
        .execute(&mut connection)
        .is_err();

        Ok(serde_json::json!({
            "schema": "ores.orm-data-plane-witness/v1",
            "lane": "__LANE__",
            "authority": "__AUTHORITY__",
            "orm": "diesel",
            "witness": {
                "rows": [normalize(&full_read), normalize(&null_read)],
                "rejections": {
                    "primaryKey": primary_key,
                    "uniqueKey": unique_key,
                    "enumCheck": enum_check,
                    "requiredNotNull": required_not_null,
                    "int32Overflow": int32_overflow,
                    "invalidTimestamp": invalid_timestamp,
                }
            }
        }))
    }
}
'''
    return (
        template.replace("__MODULE__", module_name)
        .replace("__TABLE__", model.table)
        .replace("__FIELDS__", fields)
        .replace("__STRUCT_FIELDS__", struct_fields)
        .replace("__SCHEMA__", schema)
        .replace("__LANE__", f"{authority}-diesel")
        .replace("__AUTHORITY__", authority)
    )


def seaorm_data_plane_module(
    module_name: str,
    model: Model,
    authority: str,
    schema: str,
) -> str:
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
    template = r'''mod __MODULE__ {
    use chrono::SecondsFormat;
    use sea_orm::entity::prelude::*;
    use sea_orm::{
        ActiveModelTrait, ActiveValue::Set, ConnectOptions, ConnectionTrait,
        Database, EntityTrait,
    };

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "__TABLE__")]
    pub struct Model {
__FIELDS__
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}

    fn instant(value: &str) -> DateTimeUtc {
        chrono::DateTime::parse_from_rfc3339(value)
            .expect("checked witness timestamp")
            .with_timezone(&chrono::Utc)
    }

    fn record(
        id: &str,
        key: &str,
        status: &str,
        response_status: Option<i32>,
        response_body: Option<&str>,
    ) -> ActiveModel {
        ActiveModel {
            id: Set(id.to_owned()),
            tenant_id: Set("tenant-a".to_owned()),
            idempotency_key: Set(key.to_owned()),
            request_hash: Set(format!("hash-{id}")),
            status: Set(status.to_owned()),
            response_status: Set(response_status),
            response_body: Set(response_body.map(str::to_owned)),
            created_at: Set(instant("2026-09-03T20:00:00Z")),
            expires_at: Set(instant("2026-09-04T20:00:00Z")),
        }
    }

    fn normalize(value: &Model) -> serde_json::Value {
        serde_json::json!({
            "id": &value.id,
            "tenantId": &value.tenant_id,
            "idempotencyKey": &value.idempotency_key,
            "requestHash": &value.request_hash,
            "status": &value.status,
            "responseStatus": value.response_status,
            "responseBody": value.response_body.as_deref(),
            "createdAt": value.created_at.to_rfc3339_opts(SecondsFormat::Secs, true),
            "expiresAt": value.expires_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        })
    }

    pub async fn execute(database_url: &str) -> Result<serde_json::Value, String> {
        let mut options = ConnectOptions::new(database_url.to_owned());
        options.max_connections(1).min_connections(1);
        options.set_schema_search_path("__SCHEMA__");
        let database = Database::connect(options)
            .await
            .map_err(|error| format!("SeaORM connect failed: {error}"))?;
        Entity::delete_many()
            .exec(&database)
            .await
            .map_err(|error| format!("SeaORM cleanup failed: {error}"))?;

        record("row-full", "key-full", "succeeded", Some(201), Some("ok"))
            .insert(&database)
            .await
            .map_err(|error| format!("SeaORM full insert failed: {error}"))?;
        let full_read = Entity::find_by_id("row-full".to_owned())
            .one(&database)
            .await
            .map_err(|error| format!("SeaORM full read failed: {error}"))?
            .ok_or_else(|| "SeaORM full row missing after insert".to_owned())?;

        record("row-null", "key-null", "pending", None, None)
            .insert(&database)
            .await
            .map_err(|error| format!("SeaORM optional-null insert failed: {error}"))?;
        let null_read = Entity::find_by_id("row-null".to_owned())
            .one(&database)
            .await
            .map_err(|error| format!("SeaORM optional-null read failed: {error}"))?
            .ok_or_else(|| "SeaORM optional-null row missing after insert".to_owned())?;

        let primary_key = record(
            "row-full",
            "key-primary-duplicate",
            "pending",
            None,
            None,
        )
        .insert(&database)
        .await
        .is_err();

        let unique_key = record(
            "row-unique-duplicate",
            "key-full",
            "pending",
            None,
            None,
        )
        .insert(&database)
        .await
        .is_err();

        let enum_check = record(
            "row-invalid-enum",
            "key-invalid-enum",
            "unknown",
            None,
            None,
        )
        .insert(&database)
        .await
        .is_err();

        let required_not_null = database
            .execute_unprepared(
                "INSERT INTO __TABLE__ \
                 (id, tenant_id, idempotency_key, request_hash, status, response_status, \
                  response_body, created_at, expires_at) VALUES \
                 ('row-null-required', NULL, 'key-null-required', 'hash-null-required', \
                  'pending', NULL, NULL, TIMESTAMPTZ '2026-09-03T20:00:00Z', \
                  TIMESTAMPTZ '2026-09-04T20:00:00Z')",
            )
            .await
            .is_err();

        let int32_overflow = database
            .execute_unprepared(
                "INSERT INTO __TABLE__ \
                 (id, tenant_id, idempotency_key, request_hash, status, response_status, \
                  response_body, created_at, expires_at) VALUES \
                 ('row-int32-overflow', 'tenant-a', 'key-int32-overflow', \
                  'hash-int32-overflow', 'pending', 2147483648, NULL, \
                  TIMESTAMPTZ '2026-09-03T20:00:00Z', \
                  TIMESTAMPTZ '2026-09-04T20:00:00Z')",
            )
            .await
            .is_err();

        let invalid_timestamp = database
            .execute_unprepared(
                "INSERT INTO __TABLE__ \
                 (id, tenant_id, idempotency_key, request_hash, status, response_status, \
                  response_body, created_at, expires_at) VALUES \
                 ('row-invalid-time', 'tenant-a', 'key-invalid-time', \
                  'hash-invalid-time', 'pending', NULL, NULL, 'not-a-time', \
                  TIMESTAMPTZ '2026-09-04T20:00:00Z')",
            )
            .await
            .is_err();

        Ok(serde_json::json!({
            "schema": "ores.orm-data-plane-witness/v1",
            "lane": "__LANE__",
            "authority": "__AUTHORITY__",
            "orm": "seaorm",
            "witness": {
                "rows": [normalize(&full_read), normalize(&null_read)],
                "rejections": {
                    "primaryKey": primary_key,
                    "uniqueKey": unique_key,
                    "enumCheck": enum_check,
                    "requiredNotNull": required_not_null,
                    "int32Overflow": int32_overflow,
                    "invalidTimestamp": invalid_timestamp,
                }
            }
        }))
    }
}
'''
    return (
        template.replace("__MODULE__", module_name)
        .replace("__TABLE__", model.table)
        .replace("__FIELDS__", "\n".join(fields))
        .replace("__SCHEMA__", schema)
        .replace("__LANE__", f"{authority}-seaorm")
        .replace("__AUTHORITY__", authority)
    )


def render_rust_data_plane(typespec_model: Model, json_model: Model) -> str:
    modules = "\n".join(
        (
            diesel_data_plane_module(
                "typespec_diesel",
                typespec_model,
                "typespec",
                "typespec_data_plane",
            ),
            seaorm_data_plane_module(
                "typespec_seaorm",
                typespec_model,
                "typespec",
                "typespec_data_plane",
            ),
            diesel_data_plane_module(
                "json_schema_openapi_diesel",
                json_model,
                "json-schema-openapi",
                "json_schema_data_plane",
            ),
            seaorm_data_plane_module(
                "json_schema_openapi_seaorm",
                json_model,
                "json-schema-openapi",
                "json_schema_data_plane",
            ),
        )
    )
    return f'''#![forbid(unsafe_code)]

{modules}

#[tokio::main]
async fn main() {{
    let lane = std::env::args()
        .nth(1)
        .expect("one ORM data-plane lane is required");
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL is required for ORM data-plane convergence");
    let result = match lane.as_str() {{
        "typespec-diesel" => typespec_diesel::execute(&database_url),
        "typespec-seaorm" => typespec_seaorm::execute(&database_url).await,
        "json-schema-openapi-diesel" => {{
            json_schema_openapi_diesel::execute(&database_url)
        }}
        "json-schema-openapi-seaorm" => {{
            json_schema_openapi_seaorm::execute(&database_url).await
        }}
        other => Err(format!("unknown ORM data-plane lane: {{other}}")),
    }};
    match result {{
        Ok(value) => println!(
            "{{}}",
            serde_json::to_string(&value).expect("witness serializes")
        ),
        Err(error) => {{
            eprintln!("{{error}}");
            std::process::exit(2);
        }}
    }}
}}
'''


def compare_data_plane_witnesses(
    witnesses: dict[str, Any],
    findings: list[Discrepancy],
) -> None:
    expected_lanes = set(DATA_PLANE_LANES)
    if set(witnesses) != expected_lanes:
        findings.append(
            discrepancy(
                "orm-data-plane-lane-coverage-mismatch",
                f"expected={sorted(expected_lanes)!r}; actual={sorted(witnesses)!r}",
            )
        )
        return

    normalized: dict[str, Any] = {}
    for lane, (authority, orm, _) in DATA_PLANE_LANES.items():
        value = witnesses[lane]
        if value.get("schema") != "ores.orm-data-plane-witness/v1":
            findings.append(
                discrepancy(
                    "orm-data-plane-witness-schema-mismatch",
                    f"lane={lane}; schema={value.get('schema')!r}",
                )
            )
        if value.get("authority") != authority or value.get("orm") != orm:
            findings.append(
                discrepancy(
                    "orm-data-plane-witness-identity-mismatch",
                    f"lane={lane}; authority={value.get('authority')!r}; "
                    f"orm={value.get('orm')!r}",
                )
            )
        witness = value.get("witness")
        if not isinstance(witness, dict):
            findings.append(
                discrepancy(
                    "orm-data-plane-witness-missing",
                    f"lane={lane}; witness={witness!r}",
                )
            )
            continue
        rejections = witness.get("rejections")
        if not isinstance(rejections, dict):
            findings.append(
                discrepancy(
                    "orm-data-plane-rejections-missing",
                    f"lane={lane}; rejections={rejections!r}",
                )
            )
        else:
            for rejection in EXPECTED_REJECTIONS:
                if rejections.get(rejection) is not True:
                    findings.append(
                        discrepancy(
                            "orm-data-plane-negative-case-accepted",
                            f"lane={lane}; case={rejection}; "
                            f"value={rejections.get(rejection)!r}",
                        )
                    )
        rows = witness.get("rows")
        if not isinstance(rows, list) or len(rows) != 2:
            findings.append(
                discrepancy(
                    "orm-data-plane-row-coverage-mismatch",
                    f"lane={lane}; rows={rows!r}",
                )
            )
        normalized[lane] = witness

    baseline_lane = next(iter(DATA_PLANE_LANES))
    baseline = normalized.get(baseline_lane)
    if baseline is None:
        return
    for lane in DATA_PLANE_LANES:
        if lane == baseline_lane or lane not in normalized:
            continue
        compare(
            f"ORM data-plane normalized witness ({baseline_lane} vs {lane})",
            baseline,
            normalized[lane],
            "orm-data-plane-cross-lane-mismatch",
            findings,
        )


def build_and_execute(
    root: Path,
    output_root: Path,
    database_url: str,
    typespec_model: Model,
    json_model: Model,
) -> tuple[dict[str, Any], dict[str, str], Path]:
    crate = output_root / "rust-data-plane"
    write_text(crate / "Cargo.toml", render_data_plane_cargo())
    write_text(
        crate / "src/main.rs",
        render_rust_data_plane(typespec_model, json_model),
    )
    cargo = shutil.which("cargo")
    if not cargo:
        raise ValueError("cargo is required for ORM data-plane convergence")
    env = {
        "CARGO_TARGET_DIR": str(output_root / "cargo-target"),
        "DATABASE_URL": database_url,
    }
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

    witnesses: dict[str, Any] = {}
    for lane in DATA_PLANE_LANES:
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
        witnesses[lane] = json.loads(output)
        write_json(output_root / "witnesses" / f"{lane}.json", witnesses[lane])

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
    return witnesses, versions, crate


def run(
    root: Path = ROOT,
    output_root: Path | None = None,
    database_url: str | None = None,
) -> tuple[list[Discrepancy], dict[str, Any]]:
    root = root.resolve()
    output_root = resolve_cli_path(
        root,
        output_root,
        root / "target" / "orm-data-plane-gate",
    )
    database_url = database_url or os.environ.get("DATABASE_URL")
    if not database_url:
        raise ValueError("DATABASE_URL or --database-url is required")
    started = now()
    findings: list[Discrepancy] = []

    typespec_model = parse_typespec(root)
    json_model = parse_json_schema(root)
    validate_supported_model(typespec_model, "typespec")
    validate_supported_model(json_model, "json-schema-openapi")

    sql_typespec = render_sql(typespec_model)
    sql_json = render_sql(json_model)
    typespec_sql_path = output_root / "sql/typespec.sql"
    json_sql_path = output_root / "sql/json-schema-openapi.sql"
    write_text(typespec_sql_path, sql_typespec)
    write_text(json_sql_path, sql_json)
    apply_lane(database_url, "typespec_data_plane", sql_typespec)
    apply_lane(database_url, "json_schema_data_plane", sql_json)

    witnesses, versions, crate = build_and_execute(
        root,
        output_root,
        database_url,
        typespec_model,
        json_model,
    )
    compare_data_plane_witnesses(witnesses, findings)

    artifact_paths = [
        crate / "Cargo.toml",
        crate / "Cargo.lock",
        crate / "src/main.rs",
        typespec_sql_path,
        json_sql_path,
        *(
            output_root / "witnesses" / f"{lane}.json"
            for lane in DATA_PLANE_LANES
        ),
    ]
    artifacts = {
        path.relative_to(root).as_posix(): sha256_bytes(path.read_bytes())
        for path in artifact_paths
    }
    report = {
        "schema": "ores.orm-data-plane-convergence-report/v1",
        "startedAt": started,
        "endedAt": now(),
        "actor": os.environ.get("GITHUB_ACTOR")
        or os.environ.get("USER")
        or "unknown",
        "scope": {
            "commit": os.environ.get("GITHUB_SHA"),
            "sourceDigests": source_digests(root),
            "database": redact_database_url(database_url),
            "schemas": ["typespec_data_plane", "json_schema_data_plane"],
        },
        "authorities": ["typespec", "json-schema-openapi"],
        "lanes": {
            lane: {
                "authority": authority,
                "orm": orm,
                "schema": schema,
            }
            for lane, (authority, orm, schema) in DATA_PLANE_LANES.items()
        },
        "crossChecks": [
            "TypeSpec SQL + Diesel insert/read/rejection",
            "TypeSpec SQL + SeaORM insert/read/rejection",
            "JSON Schema/OpenAPI SQL + Diesel insert/read/rejection",
            "JSON Schema/OpenAPI SQL + SeaORM insert/read/rejection",
            "normalized rows and negative cases across all four lanes",
        ],
        "tools": {
            "python": sys.version.split()[0],
            "cargo": tool_version(["cargo", "--version"]),
            "rustc": tool_version(["rustc", "--version"]),
            "diesel": versions.get("diesel", "unresolved"),
            "sea-orm": versions.get("sea-orm", "unresolved"),
        },
        "witnesses": witnesses,
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
    output_root = resolve_cli_path(
        root,
        args.output_root,
        root / "target" / "orm-data-plane-gate",
    )
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
            "orm-data-plane-gate-failure",
            f"{type(exc).__name__}: {exc}",
        )
        findings = [item]
        report = {
            "schema": "ores.orm-data-plane-convergence-report/v1",
            "startedAt": started,
            "endedAt": now(),
            "actor": os.environ.get("GITHUB_ACTOR")
            or os.environ.get("USER")
            or "unknown",
            "status": "stopped_for_evaluation",
            "zeroUnexplainedFindings": False,
            "discrepancies": [asdict(item)],
        }
    report_path = resolve_cli_path(
        root,
        args.report,
        output_root / "receipt.json",
    )
    write_json(report_path, report)
    if findings:
        print(
            f"STOPPED_FOR_EVALUATION: {len(findings)} ORM data-plane "
            f"discrepancy(s); report={report_path}"
        )
        for item in findings:
            print(f"- {item.fingerprint}: {item.kind}: {item.detail}")
        return 2
    print(
        "four-way TypeSpec/JSON-Schema x Diesel/SeaORM row-level "
        f"convergence passed; report={report_path}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
