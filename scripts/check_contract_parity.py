#!/usr/bin/env python3
"""Compare the independently authored TypeSpec and JSON Schema contracts."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from dataclasses import dataclass, asdict
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
TSP_PATH = ROOT / "contracts" / "docs-serving.tsp"
SCHEMA_PATH = ROOT / "contracts" / "docs-serving.schema.json"
TOPOLOGY_PATH = ROOT / "contracts" / "authority-topology.json"


@dataclass(frozen=True)
class Discrepancy:
    fingerprint: str
    kind: str
    detail: str
    owner: str = "ORESoftware/ores-middleware"
    resolutionState: str = "unexplained"


def _fingerprint(kind: str, detail: str) -> str:
    return hashlib.sha256(f"{kind}\0{detail}".encode("utf-8")).hexdigest()


def _discrepancy(kind: str, detail: str) -> Discrepancy:
    return Discrepancy(_fingerprint(kind, detail), kind, detail)


def _extract_block(source: str, keyword: str, name: str) -> str:
    match = re.search(
        rf"\b{re.escape(keyword)}\s+{re.escape(name)}\s*\{{(?P<body>.*?)\}}",
        source,
        re.DOTALL,
    )
    if not match:
        raise ValueError(f"missing TypeSpec {keyword} {name}")
    return match.group("body")


def parse_tsp_enum(source: str, name: str) -> list[str]:
    body = _extract_block(source, "enum", name)
    values: list[str] = []
    for raw in body.splitlines():
        line = raw.strip()
        if not line or line.startswith("//"):
            continue
        match = re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*\s*:\s*\"([^\"]+)\"\s*,?", line)
        if not match:
            raise ValueError(f"unsupported TypeSpec enum member in {name}: {line}")
        values.append(match.group(1))
    return values


def _normalize_tsp_type(type_name: str) -> dict[str, Any]:
    type_name = type_name.strip()
    if type_name == "string":
        return {"type": "string"}
    if type_name == "boolean":
        return {"type": "boolean"}
    if type_name == "uint16":
        return {"type": "integer", "minimum": 0, "maximum": 65535}
    if type_name == "Record<string>":
        return {"type": "object", "additionalProperties": {"type": "string"}}
    if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", type_name):
        return {"ref": type_name}
    raise ValueError(f"unsupported TypeSpec type: {type_name}")


def parse_tsp_model(source: str, name: str) -> dict[str, dict[str, Any]]:
    body = _extract_block(source, "model", name)
    properties: dict[str, dict[str, Any]] = {}
    for raw in body.splitlines():
        line = raw.strip()
        if not line or line.startswith("//"):
            continue
        match = re.fullmatch(
            r"([A-Za-z_][A-Za-z0-9_]*)(\?)?\s*:\s*([^;]+);", line
        )
        if not match:
            raise ValueError(f"unsupported TypeSpec property in {name}: {line}")
        prop_name, optional, type_name = match.groups()
        properties[prop_name] = {
            "required": optional is None,
            "shape": _normalize_tsp_type(type_name),
        }
    return properties


def _normalize_json_shape(value: dict[str, Any]) -> dict[str, Any]:
    if "$ref" in value:
        return {"ref": str(value["$ref"]).rsplit("/", 1)[-1]}
    value_type = value.get("type")
    if value_type == "integer":
        result: dict[str, Any] = {"type": "integer"}
        if "minimum" in value:
            result["minimum"] = value["minimum"]
        if "maximum" in value:
            result["maximum"] = value["maximum"]
        return result
    if value_type in {"string", "boolean"}:
        return {"type": value_type}
    if value_type == "object":
        additional = value.get("additionalProperties")
        if isinstance(additional, dict):
            return {
                "type": "object",
                "additionalProperties": _normalize_json_shape(additional),
            }
        return {"type": "object"}
    raise ValueError(f"unsupported JSON Schema property shape: {value!r}")


def parse_json_model(schema: dict[str, Any], name: str) -> dict[str, dict[str, Any]]:
    definition = schema["$defs"][name]
    required = set(definition.get("required", []))
    return {
        prop_name: {
            "required": prop_name in required,
            "shape": _normalize_json_shape(prop_schema),
        }
        for prop_name, prop_schema in definition["properties"].items()
    }


def _compare(label: str, left: Any, right: Any, out: list[Discrepancy]) -> None:
    if left != right:
        left_text = json.dumps(left, sort_keys=True, separators=(",", ":"))
        right_text = json.dumps(right, sort_keys=True, separators=(",", ":"))
        out.append(
            _discrepancy(
                "peer-contract-mismatch",
                f"{label}: TypeSpec={left_text}; JSON-Schema={right_text}",
            )
        )


def check_topology(topology: dict[str, Any]) -> list[Discrepancy]:
    discrepancies: list[Discrepancy] = []
    authorities = topology.get("authorities")
    expected_ids = {"typespec", "json-schema-openapi"}
    actual_ids = {
        item.get("id")
        for item in authorities or []
        if item.get("kind") == "human-authored" and item.get("topLevel") is True
    }
    if actual_ids != expected_ids:
        discrepancies.append(
            _discrepancy(
                "authority-topology",
                f"top-level human-authored authorities must be {sorted(expected_ids)}, got {sorted(actual_ids)}",
            )
        )

    expected_flows = {
        "typespec": ["sql-when-applicable", "protobuf", "grpc", "wire-clients"],
        "json-schema-openapi": [
            "interfaces-types",
            "sql-when-applicable",
            "write-clients",
        ],
    }
    if topology.get("flows") != expected_flows:
        discrepancies.append(
            _discrepancy(
                "authority-topology",
                "authority flows do not match the required peer TypeSpec and JSON Schema/OpenAPI lanes",
            )
        )

    prohibited = {tuple(edge) for edge in topology.get("prohibitedAuthorityEdges", [])}
    expected_prohibited = {
        ("typespec", "json-schema-openapi"),
        ("json-schema-openapi", "typespec"),
    }
    if prohibited != expected_prohibited:
        discrepancies.append(
            _discrepancy(
                "authority-topology",
                "both cross-authority precedence edges must be explicitly prohibited",
            )
        )

    if topology.get("onUnexplainedMismatch") != "STOPPED_FOR_EVALUATION":
        discrepancies.append(
            _discrepancy(
                "authority-topology",
                "unexplained mismatches must enter STOPPED_FOR_EVALUATION",
            )
        )
    return discrepancies


def run(root: Path = ROOT) -> list[Discrepancy]:
    tsp_path = root / "contracts" / "docs-serving.tsp"
    schema_path = root / "contracts" / "docs-serving.schema.json"
    topology_path = root / "contracts" / "authority-topology.json"

    tsp = tsp_path.read_text(encoding="utf-8")
    schema = json.loads(schema_path.read_text(encoding="utf-8"))
    topology = json.loads(topology_path.read_text(encoding="utf-8"))

    discrepancies = check_topology(topology)
    for enum_name in ("DocsRepresentation", "DocsAction"):
        _compare(
            f"enum {enum_name}",
            parse_tsp_enum(tsp, enum_name),
            schema["$defs"][enum_name]["enum"],
            discrepancies,
        )
    for model_name in ("DocsRequest", "DocsDecision"):
        _compare(
            f"model {model_name}",
            parse_tsp_model(tsp, model_name),
            parse_json_model(schema, model_name),
            discrepancies,
        )
    return discrepancies


def _write_report(path: Path, discrepancies: list[Discrepancy]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    report = {
        "schema": "ores.contract-parity-report/v1",
        "authorities": ["typespec", "json-schema-openapi"],
        "status": "stopped_for_evaluation" if discrepancies else "passed",
        "zeroUnexplainedFindings": not discrepancies,
        "discrepancies": [asdict(item) for item in discrepancies],
    }
    path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--report",
        type=Path,
        default=None,
        help="Output path; defaults to target/audit or target/discrepancies.",
    )
    args = parser.parse_args()

    try:
        discrepancies = run()
    except (OSError, ValueError, KeyError, json.JSONDecodeError) as exc:
        discrepancies = [_discrepancy("contract-check-failure", str(exc))]

    output = args.report
    if output is None:
        folder = "discrepancies" if discrepancies else "audit"
        output = ROOT / "target" / folder / "docs-serving-contract-parity.json"
    _write_report(output, discrepancies)

    if discrepancies:
        print(f"STOPPED_FOR_EVALUATION: {len(discrepancies)} discrepancy(s); report={output}")
        for item in discrepancies:
            print(f"- {item.fingerprint}: {item.detail}")
        return 2
    print(f"peer contract parity passed; report={output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
