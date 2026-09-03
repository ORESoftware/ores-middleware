#!/usr/bin/env python3
"""Bidirectional, non-authoritative TypeSpec/JSON Schema translation witnesses.

TypeSpec and JSON Schema/OpenAPI remain independent, human-authored, top-level
contract authorities. This module never rewrites either source. It renders
shadow translations in both directions, parses those shadows back, and compares
semantic models and two round trips. Any unsupported construct or unexplained
mismatch returns STOPPED_FOR_EVALUATION (exit code 2).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Iterable

ROOT = Path(__file__).resolve().parents[1]
TSP_REL = Path("contracts/persistence/idempotency-record.tsp")
JSON_REL = Path("contracts/persistence/idempotency-record.schema.json")
NAMESPACE = "Ores.Middleware.Persistence"


@dataclass(frozen=True)
class Field:
    name: str
    logical_type: str
    nullable: bool
    enum_name: str | None = None
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


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def fingerprint(kind: str, detail: str) -> str:
    return sha256_bytes(f"{kind}\0{detail}".encode())


def discrepancy(kind: str, detail: str) -> Discrepancy:
    return Discrepancy(fingerprint(kind, detail), kind, detail)


def snake_case(value: str) -> str:
    return re.sub(r"(?<!^)(?=[A-Z])", "_", value).lower()


def metadata(source: str, key: str) -> str:
    match = re.search(
        rf"^\s*//\s*@ores\.sql\.{re.escape(key)}\s+(.+?)\s*$",
        source,
        re.MULTILINE,
    )
    if not match:
        raise ValueError(f"missing TypeSpec SQL metadata {key}")
    return match.group(1)


def parse_typespec_source(source: str) -> Model:
    namespace = re.search(r"\bnamespace\s+([A-Za-z_][A-Za-z0-9_.]*)\s*;", source)
    if not namespace:
        raise ValueError("missing TypeSpec namespace")

    enum_blocks = re.findall(
        r"\benum\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{(.*?)\}", source, re.DOTALL
    )
    enums: dict[str, tuple[str, ...]] = {}
    for enum_name, body in enum_blocks:
        values: list[str] = []
        for raw in body.splitlines():
            line = raw.strip()
            if not line or line.startswith("//"):
                continue
            match = re.fullmatch(
                r'[A-Za-z_][A-Za-z0-9_]*\s*:\s*"([^"]+)"\s*,?', line
            )
            if not match:
                raise ValueError(f"unsupported TypeSpec enum member: {line}")
            values.append(match.group(1))
        if not values:
            raise ValueError(f"TypeSpec enum {enum_name} is empty")
        enums[enum_name] = tuple(values)

    model_matches = re.findall(
        r"\bmodel\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{(.*?)\}", source, re.DOTALL
    )
    if len(model_matches) != 1:
        raise ValueError(
            f"supported translation subset requires exactly one model; got {len(model_matches)}"
        )
    model_name, body = model_matches[0]
    fields: list[Field] = []
    for raw in body.splitlines():
        line = raw.strip()
        if not line or line.startswith("//"):
            continue
        match = re.fullmatch(
            r"([A-Za-z_][A-Za-z0-9_]*)(\?)?\s*:\s*([^;]+);", line
        )
        if not match:
            raise ValueError(f"unsupported TypeSpec field: {line}")
        name, optional, raw_type = match.groups()
        type_name = raw_type.strip()
        if type_name in {"string", "int32", "utcDateTime"}:
            fields.append(Field(name, type_name, optional is not None))
        elif type_name in enums:
            fields.append(
                Field(name, "enum", optional is not None, type_name, enums[type_name])
            )
        else:
            raise ValueError(f"unsupported TypeSpec field type {type_name!r}")

    primary = tuple(
        item.strip() for item in metadata(source, "primary-key").split(",") if item.strip()
    )
    unique = tuple(
        tuple(item.strip() for item in group.split(",") if item.strip())
        for group in metadata(source, "unique").split(";")
        if group.strip()
    )
    known = {field.name for field in fields}
    referenced = set(primary)
    for group in unique:
        referenced.update(group)
    missing = sorted(referenced - known)
    if missing:
        raise ValueError(f"SQL metadata references missing fields: {', '.join(missing)}")
    return Model(model_name, metadata(source, "table"), primary, unique, tuple(fields))


def parse_json_schema_document(doc: dict[str, Any]) -> Model:
    defs = doc.get("$defs")
    if not isinstance(defs, dict):
        raise ValueError("JSON Schema must contain $defs")
    root_ref = doc.get("$ref")
    if not isinstance(root_ref, str) or not root_ref.startswith("#/$defs/"):
        raise ValueError("JSON Schema root must reference one model in $defs")
    model_name = root_ref.rsplit("/", 1)[-1]
    model = defs.get(model_name)
    if not isinstance(model, dict) or model.get("type") != "object":
        raise ValueError(f"missing object definition {model_name}")
    if model.get("additionalProperties") is not False:
        raise ValueError("supported translation subset requires additionalProperties=false")
    properties = model.get("properties")
    if not isinstance(properties, dict):
        raise ValueError("object definition must contain properties")
    required_raw = model.get("required", [])
    if not isinstance(required_raw, list) or not all(
        isinstance(item, str) for item in required_raw
    ):
        raise ValueError("required must be a string array")
    required = set(required_raw)
    unknown_required = sorted(required - set(properties))
    if unknown_required:
        raise ValueError(f"required references missing properties: {unknown_required}")

    fields: list[Field] = []
    for name, prop in properties.items():
        if not isinstance(name, str) or not isinstance(prop, dict):
            raise ValueError("properties must map names to schemas")
        nullable = name not in required
        if "$ref" in prop:
            ref = prop["$ref"]
            if not isinstance(ref, str) or not ref.startswith("#/$defs/"):
                raise ValueError(f"unsupported JSON Schema ref for {name}")
            enum_name = ref.rsplit("/", 1)[-1]
            enum_doc = defs.get(enum_name)
            if not isinstance(enum_doc, dict) or enum_doc.get("type") != "string":
                raise ValueError(f"missing string enum definition {enum_name}")
            values = enum_doc.get("enum")
            if not isinstance(values, list) or not values or not all(
                isinstance(item, str) for item in values
            ):
                raise ValueError(f"enum {enum_name} must contain strings")
            fields.append(Field(name, "enum", nullable, enum_name, tuple(values)))
        elif prop.get("type") == "string" and prop.get("format") == "date-time":
            fields.append(Field(name, "utcDateTime", nullable))
        elif prop.get("type") == "string" and "format" not in prop:
            fields.append(Field(name, "string", nullable))
        elif (
            prop.get("type") == "integer"
            and prop.get("minimum") == -2147483648
            and prop.get("maximum") == 2147483647
        ):
            fields.append(Field(name, "int32", nullable))
        else:
            raise ValueError(f"unsupported JSON Schema property {name}: {prop!r}")

    sql = model.get("x-ores-sql")
    if not isinstance(sql, dict):
        raise ValueError("object definition must contain x-ores-sql")
    table = sql.get("table")
    primary = sql.get("primaryKey")
    unique = sql.get("unique")
    if not isinstance(table, str) or not table:
        raise ValueError("x-ores-sql.table must be a non-empty string")
    if not isinstance(primary, list) or not all(isinstance(x, str) for x in primary):
        raise ValueError("x-ores-sql.primaryKey must be a string array")
    if not isinstance(unique, list) or not all(
        isinstance(group, list) and all(isinstance(x, str) for x in group)
        for group in unique
    ):
        raise ValueError("x-ores-sql.unique must be an array of string arrays")
    known = {field.name for field in fields}
    referenced = set(primary)
    for group in unique:
        referenced.update(group)
    missing = sorted(referenced - known)
    if missing:
        raise ValueError(f"x-ores-sql references missing properties: {missing}")
    return Model(
        model_name,
        table,
        tuple(primary),
        tuple(tuple(group) for group in unique),
        tuple(fields),
    )


def canonical(model: Model) -> dict[str, Any]:
    names = {field.name: snake_case(field.name) for field in model.fields}
    return {
        "name": model.name,
        "table": model.table,
        "primaryKey": [names[name] for name in model.primary_key],
        "unique": [[names[name] for name in group] for group in model.unique],
        "fields": [
            {
                "name": field.name,
                "column": names[field.name],
                "logicalType": field.logical_type,
                "nullable": field.nullable,
                "enumName": field.enum_name,
                "enumValues": list(field.enum_values),
            }
            for field in model.fields
        ],
    }


def json_property(field: Field) -> dict[str, Any]:
    if field.logical_type == "string":
        return {"type": "string"}
    if field.logical_type == "utcDateTime":
        return {"type": "string", "format": "date-time"}
    if field.logical_type == "int32":
        return {
            "type": "integer",
            "minimum": -2147483648,
            "maximum": 2147483647,
        }
    if field.logical_type == "enum" and field.enum_name:
        return {"$ref": f"#/$defs/{field.enum_name}"}
    raise ValueError(f"cannot render JSON Schema type {field.logical_type!r}")


def render_json_schema(model: Model, *, source_authority: str, source_digest: str) -> dict[str, Any]:
    enum_defs: dict[str, Any] = {}
    for field in model.fields:
        if field.logical_type != "enum" or not field.enum_name:
            continue
        current = enum_defs.get(field.enum_name)
        value = {"type": "string", "enum": list(field.enum_values)}
        if current is not None and current != value:
            raise ValueError(f"inconsistent values for enum {field.enum_name}")
        enum_defs[field.enum_name] = value
    model_def = {
        "type": "object",
        "additionalProperties": False,
        "x-ores-sql": {
            "table": model.table,
            "primaryKey": list(model.primary_key),
            "unique": [list(group) for group in model.unique],
        },
        "properties": {field.name: json_property(field) for field in model.fields},
        "required": [field.name for field in model.fields if not field.nullable],
    }
    return {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": f"urn:ores:shadow:{source_authority}:{model.name}",
        "$comment": (
            "NON-AUTHORITATIVE GENERATED WITNESS. Never replace or rewrite the "
            "independent human-authored JSON Schema/OpenAPI authority from this file."
        ),
        "x-ores-witness": {
            "authoritative": False,
            "sourceAuthority": source_authority,
            "sourceDigest": source_digest,
            "purpose": "cross-translation-and-round-trip-convergence-only",
        },
        "$ref": f"#/$defs/{model.name}",
        "$defs": {**enum_defs, model.name: model_def},
    }


def tsp_identifier(value: str) -> str:
    candidate = re.sub(r"[^A-Za-z0-9_]", "_", value)
    if not candidate or not re.match(r"[A-Za-z_]", candidate):
        candidate = "value_" + candidate
    return candidate


def render_typespec(model: Model, *, source_authority: str, source_digest: str) -> str:
    lines = [
        "// NON-AUTHORITATIVE GENERATED WITNESS.",
        "// Never replace or rewrite the independent human-authored TypeSpec authority from this file.",
        f"// @ores.witness.source-authority {source_authority}",
        f"// @ores.witness.source-digest {source_digest}",
        "// @ores.witness.purpose cross-translation-and-round-trip-convergence-only",
        f"namespace {NAMESPACE};",
        "",
        f"// @ores.sql.table {model.table}",
        f"// @ores.sql.primary-key {','.join(model.primary_key)}",
        "// @ores.sql.unique " + ";".join(",".join(group) for group in model.unique),
        "",
    ]
    emitted: set[str] = set()
    for field in model.fields:
        if field.logical_type != "enum" or not field.enum_name or field.enum_name in emitted:
            continue
        emitted.add(field.enum_name)
        lines.append(f"enum {field.enum_name} {{")
        for value in field.enum_values:
            lines.append(f"  {tsp_identifier(value)}: {json.dumps(value)},")
        lines.extend(["}", ""])
    lines.append(f"model {model.name} {{")
    for field in model.fields:
        optional = "?" if field.nullable else ""
        type_name = field.enum_name if field.logical_type == "enum" else field.logical_type
        if not type_name:
            raise ValueError(f"missing TypeSpec type for {field.name}")
        lines.append(f"  {field.name}{optional}: {type_name};")
    lines.extend(["}", ""])
    return "\n".join(lines)


def compare_models(
    label: str, left: Model, right: Model, kind: str, findings: list[Discrepancy]
) -> None:
    left_value = canonical(left)
    right_value = canonical(right)
    if left_value != right_value:
        findings.append(
            discrepancy(
                kind,
                f"{label}: left={json.dumps(left_value, sort_keys=True)}; "
                f"right={json.dumps(right_value, sort_keys=True)}",
            )
        )


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_text(path: Path, value: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(value, encoding="utf-8")


def validate_json_schema(document: dict[str, Any]) -> str:
    try:
        from jsonschema import Draft202012Validator
    except ModuleNotFoundError:
        return "jsonschema unavailable; semantic parse completed"
    Draft202012Validator.check_schema(document)
    return "Draft 2020-12 meta-schema validation passed"


def compile_typespec(paths: Iterable[Path], root: Path) -> dict[str, str]:
    tsp_bin = os.environ.get("TSP_BIN") or shutil.which("tsp")
    if not tsp_bin:
        return {path.name: "tsp unavailable; semantic parse completed" for path in paths}
    result: dict[str, str] = {}
    for path in paths:
        completed = subprocess.run(
            [tsp_bin, "compile", str(path), "--no-emit"],
            cwd=root,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=False,
        )
        if completed.returncode:
            raise ValueError(
                f"generated TypeSpec witness failed to compile ({path}): "
                f"{completed.stdout.strip()}"
            )
        result[path.name] = "compiled"
    return result


def run(
    root: Path = ROOT, output_root: Path | None = None
) -> tuple[list[Discrepancy], dict[str, Any]]:
    output_root = output_root or root / "target" / "cross-translation"
    tsp_path = root / TSP_REL
    json_path = root / JSON_REL
    tsp_bytes = tsp_path.read_bytes()
    json_bytes = json_path.read_bytes()
    tsp_digest = sha256_bytes(tsp_bytes)
    json_digest = sha256_bytes(json_bytes)
    tsp_model = parse_typespec_source(tsp_bytes.decode("utf-8"))
    json_model = parse_json_schema_document(json.loads(json_bytes))

    tsp_to_json_doc = render_json_schema(
        tsp_model, source_authority="typespec", source_digest=tsp_digest
    )
    json_to_tsp_text = render_typespec(
        json_model, source_authority="json-schema-openapi", source_digest=json_digest
    )
    tsp_to_json_path = output_root / "typespec-to-json-schema" / "idempotency-record.shadow.schema.json"
    json_to_tsp_path = output_root / "json-schema-to-typespec" / "idempotency-record.shadow.tsp"
    write_json(tsp_to_json_path, tsp_to_json_doc)
    write_text(json_to_tsp_path, json_to_tsp_text)

    tsp_json_model = parse_json_schema_document(tsp_to_json_doc)
    json_tsp_model = parse_typespec_source(json_to_tsp_text)

    tsp_roundtrip_text = render_typespec(
        tsp_json_model,
        source_authority="typespec-via-json-schema-shadow",
        source_digest=sha256_bytes(tsp_to_json_path.read_bytes()),
    )
    json_roundtrip_doc = render_json_schema(
        json_tsp_model,
        source_authority="json-schema-openapi-via-typespec-shadow",
        source_digest=sha256_bytes(json_to_tsp_path.read_bytes()),
    )
    tsp_roundtrip_path = output_root / "round-trip" / "typespec-json-schema-typespec.shadow.tsp"
    json_roundtrip_path = output_root / "round-trip" / "json-schema-typespec-json-schema.shadow.schema.json"
    write_text(tsp_roundtrip_path, tsp_roundtrip_text)
    write_json(json_roundtrip_path, json_roundtrip_doc)
    tsp_roundtrip_model = parse_typespec_source(tsp_roundtrip_text)
    json_roundtrip_model = parse_json_schema_document(json_roundtrip_doc)

    findings: list[Discrepancy] = []
    compare_models(
        "independent authority semantic parity",
        tsp_model,
        json_model,
        "peer-authority-cross-translation-mismatch",
        findings,
    )
    compare_models(
        "TypeSpec -> JSON Schema shadow",
        tsp_model,
        tsp_json_model,
        "typespec-to-json-schema-loss",
        findings,
    )
    compare_models(
        "JSON Schema/OpenAPI -> TypeSpec shadow",
        json_model,
        json_tsp_model,
        "json-schema-to-typespec-loss",
        findings,
    )
    compare_models(
        "TypeSpec -> JSON Schema -> TypeSpec round trip",
        tsp_model,
        tsp_roundtrip_model,
        "typespec-json-schema-typespec-roundtrip-loss",
        findings,
    )
    compare_models(
        "JSON Schema -> TypeSpec -> JSON Schema round trip",
        json_model,
        json_roundtrip_model,
        "json-schema-typespec-json-schema-roundtrip-loss",
        findings,
    )

    validation = {
        tsp_to_json_path.relative_to(root).as_posix(): validate_json_schema(tsp_to_json_doc),
        json_roundtrip_path.relative_to(root).as_posix(): validate_json_schema(json_roundtrip_doc),
    }
    compile_witnesses = compile_typespec([json_to_tsp_path, tsp_roundtrip_path], root)
    artifacts = {}
    for path in (tsp_to_json_path, json_to_tsp_path, tsp_roundtrip_path, json_roundtrip_path):
        artifacts[path.relative_to(root).as_posix()] = sha256_bytes(path.read_bytes())

    report = {
        "schema": "ores.cross-translation-report/v1",
        "authorities": {
            "typespec": {"path": TSP_REL.as_posix(), "digest": tsp_digest, "topLevel": True},
            "json-schema-openapi": {"path": JSON_REL.as_posix(), "digest": json_digest, "topLevel": True},
        },
        "generatedWitnessPolicy": {
            "authoritative": False,
            "mayRewriteHumanAuthoredSource": False,
            "purpose": "cross-translation-and-round-trip-convergence-only",
        },
        "comparisons": [
            "independent-authority-semantic-parity",
            "typespec-to-json-schema-shadow",
            "json-schema-openapi-to-typespec-shadow",
            "typespec-json-schema-typespec-round-trip",
            "json-schema-typespec-json-schema-round-trip",
        ],
        "metaSchemaValidation": validation,
        "typespecCompilation": compile_witnesses,
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
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    output = args.output_root or root / "target" / "cross-translation"
    try:
        findings, report = run(root, output)
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError) as exc:
        item = discrepancy("cross-translation-check-failure", str(exc))
        findings = [item]
        report = {
            "schema": "ores.cross-translation-report/v1",
            "generatedWitnessPolicy": {
                "authoritative": False,
                "mayRewriteHumanAuthoredSource": False,
            },
            "status": "stopped_for_evaluation",
            "zeroUnexplainedFindings": False,
            "discrepancies": [asdict(item)],
        }
    report_path = args.report or root / "target" / (
        "discrepancies" if findings else "audit"
    ) / "cross-translation.json"
    write_json(report_path, report)
    if findings:
        print(
            f"STOPPED_FOR_EVALUATION: {len(findings)} cross-translation discrepancy(s); "
            f"report={report_path}"
        )
        for item in findings:
            print(f"- {item.fingerprint}: {item.kind}: {item.detail}")
        return 2
    print(f"cross-translation and round-trip convergence passed; report={report_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
