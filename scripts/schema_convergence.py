#!/usr/bin/env python3
"""Generate and compare TypeSpec and JSON Schema persistence projections.

The two source files are independent, human-authored authorities. This checker
never promotes either lane over the other. It generates SQL and client type
witnesses from each lane, generates Diesel/SeaORM-shaped Rust witnesses, and
returns exit code 2 (STOPPED_FOR_EVALUATION) for every unexplained mismatch.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
TSP_REL = Path("contracts/persistence/idempotency-record.tsp")
JSON_REL = Path("contracts/persistence/idempotency-record.schema.json")


@dataclass(frozen=True)
class Field:
    name: str
    column: str
    logical_type: str
    sql_type: str
    nullable: bool
    enum_values: tuple[str, ...] = ()


@dataclass(frozen=True)
class Model:
    name: str
    table: str
    primary_key: tuple[str, ...]
    unique: tuple[tuple[str, ...], ...]
    fields: tuple[Field, ...]


@dataclass(frozen=True)
class Discrepancy:
    fingerprint: str
    kind: str
    detail: str
    resolutionState: str = "unexplained"


def fingerprint(kind: str, detail: str) -> str:
    return hashlib.sha256(f"{kind}\0{detail}".encode()).hexdigest()


def discrepancy(kind: str, detail: str) -> Discrepancy:
    return Discrepancy(fingerprint(kind, detail), kind, detail)


def snake_case(value: str) -> str:
    return re.sub(r"(?<!^)(?=[A-Z])", "_", value).lower()


def block(source: str, keyword: str, name: str) -> str:
    match = re.search(
        rf"\b{re.escape(keyword)}\s+{re.escape(name)}\s*\{{(?P<body>.*?)\}}",
        source,
        re.DOTALL,
    )
    if not match:
        raise ValueError(f"missing TypeSpec {keyword} {name}")
    return match.group("body")


def tsp_enum(source: str, name: str) -> tuple[str, ...]:
    values: list[str] = []
    for raw in block(source, "enum", name).splitlines():
        line = raw.strip()
        if not line or line.startswith("//"):
            continue
        match = re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*\s*:\s*"([^"]+)"\s*,?', line)
        if not match:
            raise ValueError(f"unsupported TypeSpec enum member: {line}")
        values.append(match.group(1))
    return tuple(values)


def metadata(source: str, key: str) -> str:
    match = re.search(rf"^\s*//\s*@ores\.sql\.{re.escape(key)}\s+(.+?)\s*$", source, re.MULTILINE)
    if not match:
        raise ValueError(f"missing TypeSpec SQL metadata {key}")
    return match.group(1)


def type_shape(type_name: str, enums: dict[str, tuple[str, ...]]) -> tuple[str, str, tuple[str, ...]]:
    if type_name == "string":
        return "string", "text", ()
    if type_name == "int32":
        return "int32", "integer", ()
    if type_name == "utcDateTime":
        return "datetime", "timestamptz", ()
    if type_name in enums:
        return "enum", "text", enums[type_name]
    raise ValueError(f"unsupported persistence type {type_name}")


def parse_typespec(root: Path) -> Model:
    source = (root / TSP_REL).read_text(encoding="utf-8")
    enums = {"IdempotencyStatus": tsp_enum(source, "IdempotencyStatus")}
    fields: list[Field] = []
    for raw in block(source, "model", "IdempotencyRecord").splitlines():
        line = raw.strip()
        if not line or line.startswith("//"):
            continue
        match = re.fullmatch(r"([A-Za-z_][A-Za-z0-9_]*)(\?)?\s*:\s*([^;]+);", line)
        if not match:
            raise ValueError(f"unsupported TypeSpec field: {line}")
        name, optional, raw_type = match.groups()
        logical, sql_type, enum_values = type_shape(raw_type.strip(), enums)
        fields.append(Field(name, snake_case(name), logical, sql_type, optional is not None, enum_values))
    primary = tuple(item.strip() for item in metadata(source, "primary-key").split(",") if item.strip())
    unique = tuple(
        tuple(item.strip() for item in group.split(",") if item.strip())
        for group in metadata(source, "unique").split(";")
        if group.strip()
    )
    return Model("IdempotencyRecord", metadata(source, "table"), primary, unique, tuple(fields))


def json_shape(prop: dict[str, Any], defs: dict[str, Any]) -> tuple[str, str, tuple[str, ...]]:
    if "$ref" in prop:
        name = str(prop["$ref"]).rsplit("/", 1)[-1]
        definition = defs[name]
        return "enum", "text", tuple(definition["enum"])
    if prop.get("type") == "string" and prop.get("format") == "date-time":
        return "datetime", "timestamptz", ()
    if prop.get("type") == "string":
        return "string", "text", ()
    if prop.get("type") == "integer" and prop.get("minimum") == -2147483648 and prop.get("maximum") == 2147483647:
        return "int32", "integer", ()
    raise ValueError(f"unsupported JSON Schema field: {prop!r}")


def parse_json_schema(root: Path) -> Model:
    doc = json.loads((root / JSON_REL).read_text(encoding="utf-8"))
    defs = doc["$defs"]
    model = defs["IdempotencyRecord"]
    sql = model["x-ores-sql"]
    required = set(model["required"])
    fields = []
    for name, prop in model["properties"].items():
        logical, sql_type, enum_values = json_shape(prop, defs)
        fields.append(Field(name, snake_case(name), logical, sql_type, name not in required, enum_values))
    return Model(
        "IdempotencyRecord",
        sql["table"],
        tuple(sql["primaryKey"]),
        tuple(tuple(group) for group in sql["unique"]),
        tuple(fields),
    )


def canonical(model: Model) -> dict[str, Any]:
    names = {field.name: field.column for field in model.fields}
    return {
        "name": model.name,
        "table": model.table,
        "primaryKey": [names[name] for name in model.primary_key],
        "unique": [[names[name] for name in group] for group in model.unique],
        "fields": [
            {
                "name": field.name,
                "column": field.column,
                "logicalType": field.logical_type,
                "sqlType": field.sql_type,
                "nullable": field.nullable,
                "enumValues": list(field.enum_values),
            }
            for field in model.fields
        ],
    }


def quoted(value: str) -> str:
    return '"' + value.replace('"', '""') + '"'


def sql_literal(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def render_sql(model: Model) -> str:
    names = {field.name: field.column for field in model.fields}
    lines = []
    for field in model.fields:
        nullability = "" if field.nullable else " NOT NULL"
        lines.append(f"  {quoted(field.column)} {field.sql_type}{nullability}")
    primary_columns = ", ".join(quoted(names[name]) for name in model.primary_key)
    lines.append(f"  CONSTRAINT {quoted('pk_' + model.table)} PRIMARY KEY ({primary_columns})")
    for group in model.unique:
        columns = [names[name] for name in group]
        suffix = "_".join(columns)
        rendered = ", ".join(quoted(column) for column in columns)
        lines.append(f"  CONSTRAINT {quoted('uq_' + model.table + '_' + suffix)} UNIQUE ({rendered})")
    for field in model.fields:
        if field.enum_values:
            choices = ", ".join(sql_literal(item) for item in field.enum_values)
            lines.append(
                f"  CONSTRAINT {quoted('ck_' + model.table + '_' + field.column)} "
                f"CHECK ({quoted(field.column)} IN ({choices}))"
            )
    return f"CREATE TABLE {quoted(model.table)} (\n" + ",\n".join(lines) + "\n);\n"


def ts_type(field: Field) -> str:
    if field.logical_type == "int32":
        return "number"
    if field.logical_type == "enum":
        return " | ".join(json.dumps(item) for item in field.enum_values)
    return "string"


def render_typescript(model: Model) -> str:
    lines = [f"export interface {model.name} {{"]
    for field in model.fields:
        optional = "?" if field.nullable else ""
        lines.append(f"  {field.name}{optional}: {ts_type(field)};")
    lines.append("}")
    return "\n".join(lines) + "\n"


def rust_type(field: Field) -> str:
    base = "i32" if field.logical_type == "int32" else "String"
    return f"Option<{base}>" if field.nullable else base


def render_rust(model: Model, orm: str) -> str:
    struct_name = f"{orm}{model.name}"
    lines = ["#![forbid(unsafe_code)]", "", "#[derive(Clone, Debug, PartialEq, Eq)]", f"pub struct {struct_name} {{"]
    for field in model.fields:
        lines.append(f"    pub {field.column}: {rust_type(field)},")
    lines.extend(["}", ""])
    return "\n".join(lines)


def orm_manifest(model: Model, orm: str) -> dict[str, Any]:
    return {
        "orm": orm,
        "table": model.table,
        "primaryKey": list(canonical(model)["primaryKey"]),
        "unique": canonical(model)["unique"],
        "fields": [
            {"column": field.column, "rustType": rust_type(field), "nullable": field.nullable}
            for field in model.fields
        ],
    }


def write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def write_json(path: Path, content: Any) -> None:
    write_text(path, json.dumps(content, indent=2, sort_keys=True) + "\n")


def compile_rust_witness(path: Path) -> str:
    rustc = shutil.which("rustc")
    if not rustc:
        return "rustc unavailable; witness source generated but not compiled"
    output = path.with_suffix(".rlib")
    completed = subprocess.run(
        [rustc, "--crate-type", "lib", "--edition", "2021", str(path), "-o", str(output)],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    if completed.returncode:
        raise ValueError(f"Rust witness failed to compile: {completed.stdout.strip()}")
    output.unlink(missing_ok=True)
    return "compiled"


def compare(label: str, left: Any, right: Any, kind: str, out: list[Discrepancy]) -> None:
    if left != right:
        out.append(
            discrepancy(
                kind,
                f"{label}: TypeSpec={json.dumps(left, sort_keys=True)}; JSON-Schema={json.dumps(right, sort_keys=True)}",
            )
        )


def run(root: Path = ROOT, output_root: Path | None = None) -> tuple[list[Discrepancy], dict[str, Any]]:
    output_root = output_root or root / "target" / "schema-convergence"
    tsp_model = parse_typespec(root)
    json_model = parse_json_schema(root)

    tsp_dir = output_root / "typespec"
    json_dir = output_root / "json-schema-openapi"
    tsp_sql = render_sql(tsp_model)
    json_sql = render_sql(json_model)
    tsp_types = render_typescript(tsp_model)
    json_types = render_typescript(json_model)
    diesel_source = render_rust(tsp_model, "Diesel")
    seaorm_source = render_rust(json_model, "SeaOrm")
    diesel_manifest = orm_manifest(tsp_model, "diesel")
    seaorm_manifest = orm_manifest(json_model, "seaorm")

    write_json(tsp_dir / "model.json", canonical(tsp_model))
    write_json(json_dir / "model.json", canonical(json_model))
    write_text(tsp_dir / "schema.sql", tsp_sql)
    write_text(json_dir / "schema.sql", json_sql)
    write_text(tsp_dir / "types.d.ts", tsp_types)
    write_text(json_dir / "types.d.ts", json_types)
    write_text(tsp_dir / "diesel.rs", diesel_source)
    write_text(json_dir / "seaorm.rs", seaorm_source)
    write_json(tsp_dir / "diesel.manifest.json", diesel_manifest)
    write_json(json_dir / "seaorm.manifest.json", seaorm_manifest)

    compile_status = {
        "diesel": compile_rust_witness(tsp_dir / "diesel.rs"),
        "seaorm": compile_rust_witness(json_dir / "seaorm.rs"),
    }

    discrepancies: list[Discrepancy] = []
    compare("canonical model", canonical(tsp_model), canonical(json_model), "peer-contract-type-mismatch", discrepancies)
    compare("generated SQL", tsp_sql, json_sql, "generated-sql-mismatch", discrepancies)
    compare("generated client types", tsp_types, json_types, "generated-type-mismatch", discrepancies)

    diesel_semantics = {key: value for key, value in diesel_manifest.items() if key != "orm"}
    seaorm_semantics = {key: value for key, value in seaorm_manifest.items() if key != "orm"}
    compare("Diesel/SeaORM witness", diesel_semantics, seaorm_semantics, "diesel-seaorm-witness-mismatch", discrepancies)

    details = {
        "schema": "ores.schema-convergence-report/v1",
        "authorities": ["typespec", "json-schema-openapi"],
        "flows": {
            "typespec": ["sql", "protobuf", "grpc", "wire-clients"],
            "json-schema-openapi": ["interfaces-types", "sql", "write-clients"],
        },
        "compileWitnesses": compile_status,
        "status": "stopped_for_evaluation" if discrepancies else "passed",
        "zeroUnexplainedFindings": not discrepancies,
        "discrepancies": [asdict(item) for item in discrepancies],
    }
    return discrepancies, details


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--output-root", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    output_root = args.output_root or root / "target" / "schema-convergence"
    try:
        discrepancies, report = run(root, output_root)
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError) as exc:
        item = discrepancy("schema-convergence-check-failure", str(exc))
        discrepancies = [item]
        report = {
            "schema": "ores.schema-convergence-report/v1",
            "authorities": ["typespec", "json-schema-openapi"],
            "status": "stopped_for_evaluation",
            "zeroUnexplainedFindings": False,
            "discrepancies": [asdict(item)],
        }
    report_path = args.report
    if report_path is None:
        folder = "discrepancies" if discrepancies else "audit"
        report_path = root / "target" / folder / "schema-convergence.json"
    write_json(report_path, report)
    if discrepancies:
        print(f"STOPPED_FOR_EVALUATION: {len(discrepancies)} discrepancy(s); report={report_path}")
        for item in discrepancies:
            print(f"- {item.fingerprint}: {item.kind}: {item.detail}")
        return 2
    print(f"schema convergence passed; report={report_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
