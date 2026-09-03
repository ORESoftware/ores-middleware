#!/usr/bin/env python3
"""Run the peer-authority and polyglot package audit.

Every invocation writes a machine-readable receipt, including failed checks and
unexplained discrepancy fingerprints. A peer-authority or generated-artifact
mismatch returns exit code 2 and the state STOPPED_FOR_EVALUATION.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shlex
import subprocess
import sys
from dataclasses import asdict, is_dataclass
from datetime import datetime, timezone
from importlib import metadata
from pathlib import Path
from typing import Any, Callable

from jsonschema import Draft202012Validator, FormatChecker

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.check_contract_parity import run as run_contract_parity
from scripts.check_zpkg import validate as validate_zpkg
from scripts.schema_convergence import run as run_schema_convergence

RECEIPT_SCHEMA = ROOT / "contracts/schema-audit-receipt.schema.json"
SOURCE_PATHS = (
    Path(".zpkg.toml"),
    Path("contracts/authority-topology.json"),
    Path("contracts/docs-serving.tsp"),
    Path("contracts/docs-serving.schema.json"),
    Path("contracts/persistence/idempotency-record.tsp"),
    Path("contracts/persistence/idempotency-record.schema.json"),
    Path("fixtures/docs-serving-conformance.tsv"),
)


def now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def command_text(command: list[str]) -> str:
    return shlex.join(command)


def trim(value: str, limit: int = 4000) -> str:
    value = value.strip()
    if not value:
        return "passed"
    return value if len(value) <= limit else "…" + value[-limit:]


def run_command(
    check_id: str,
    command: list[str],
    *,
    cwd: Path = ROOT,
    env: dict[str, str] | None = None,
    timeout: int = 300,
) -> dict[str, Any]:
    merged = os.environ.copy()
    if env:
        merged.update(env)
    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            env=merged,
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=timeout,
        )
        return {
            "id": check_id,
            "required": True,
            "state": "executed" if completed.returncode == 0 else "failed",
            "command": command_text(command),
            "exitCode": completed.returncode,
            "detail": trim(completed.stdout),
        }
    except (OSError, subprocess.SubprocessError) as exc:
        return {
            "id": check_id,
            "required": True,
            "state": "failed",
            "command": command_text(command),
            "exitCode": 127,
            "detail": str(exc),
        }


def internal(check_id: str, fn: Callable[[], str]) -> dict[str, Any]:
    try:
        detail = fn()
    except Exception as exc:
        return {
            "id": check_id,
            "required": True,
            "state": "failed",
            "detail": f"{type(exc).__name__}: {exc}",
        }
    return {
        "id": check_id,
        "required": True,
        "state": "executed",
        "detail": detail or "passed",
    }


def normalize_discrepancy(item: Any) -> dict[str, Any]:
    value = asdict(item) if is_dataclass(item) else dict(item)
    value.setdefault("owner", "ORESoftware/ores-middleware")
    value.setdefault("resolutionState", "unexplained")
    return {
        "fingerprint": value["fingerprint"],
        "kind": value["kind"],
        "detail": value["detail"],
        "owner": value["owner"],
        "resolutionState": value["resolutionState"],
    }


def validate_schemas() -> str:
    paths = (
        "contracts/docs-serving.schema.json",
        "contracts/schema-audit-receipt.schema.json",
        "contracts/persistence/idempotency-record.schema.json",
    )
    for relative in paths:
        Draft202012Validator.check_schema(
            json.loads((ROOT / relative).read_text(encoding="utf-8"))
        )
    return "validated Draft 2020-12 schemas: " + ", ".join(paths)


def validate_zpkg_contract() -> str:
    errors = validate_zpkg(ROOT)
    if errors:
        raise ValueError("; ".join(errors))
    return (
        "repository plus six language targets and target/<language> build outputs "
        "match the peer-authority topology"
    )


def tool_version(command: list[str], fallback: str = "unavailable") -> str:
    try:
        completed = subprocess.run(
            command,
            cwd=ROOT,
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=15,
        )
    except (OSError, subprocess.SubprocessError):
        return fallback
    lines = completed.stdout.strip().splitlines()
    return lines[0] if completed.returncode == 0 and lines else fallback


def current_commit() -> str | None:
    value = os.environ.get("GITHUB_SHA")
    if value:
        return value
    try:
        completed = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=ROOT,
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            timeout=10,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    value = completed.stdout.strip()
    return value if completed.returncode == 0 and value else None


def source_files() -> list[str]:
    roots = [
        ROOT / name
        for name in (".github", "contracts", "docs", "fixtures", "scripts", "src")
    ]
    files: set[str] = {
        ".zpkg.toml",
        "AGENTS.md",
        "README.md",
        "ROLLOUT.md",
        ".gitignore",
    }
    for base in roots:
        if not base.exists():
            continue
        for path in base.rglob("*"):
            if not path.is_file():
                continue
            if any(
                part in {
                    "node_modules",
                    "target",
                    "_build",
                    "deps",
                    "__pycache__",
                }
                for part in path.parts
            ):
                continue
            files.add(path.relative_to(ROOT).as_posix())
    return sorted(item for item in files if (ROOT / item).is_file())


def receipt(
    started: str,
    checks: list[dict[str, Any]],
    discrepancies: list[dict[str, Any]],
) -> dict[str, Any]:
    required_failure = any(
        item["required"] and item["state"] != "executed" for item in checks
    )
    status = (
        "stopped_for_evaluation"
        if discrepancies
        else "failed"
        if required_failure
        else "passed"
    )
    tsp_bin = os.environ.get("TSP_BIN", "tsp")
    return {
        "schema": "ores.schema-audit-receipt/v1",
        "repository": "ORESoftware/ores-middleware",
        "startedAt": started,
        "endedAt": now(),
        "actor": os.environ.get("GITHUB_ACTOR")
        or os.environ.get("USER")
        or "unknown",
        "scope": {
            "commit": current_commit(),
            "files": source_files(),
            "sourceDigests": {
                path.as_posix(): digest(ROOT / path) for path in SOURCE_PATHS
            },
            "exclusions": [
                ".git internals",
                "generated target output",
                "language package caches",
                "credential values and deployment secrets",
                "external cloud control planes not exercised by this repository audit",
            ],
        },
        "tools": {
            "python": sys.version.split()[0],
            "jsonschema": metadata.version("jsonschema"),
            "node": tool_version(["node", "--version"]),
            "npm": tool_version(["npm", "--version"]),
            "go": tool_version(["go", "version"]),
            "cargo": tool_version(["cargo", "--version"]),
            "rustc": tool_version(["rustc", "--version"]),
            "typespec": tool_version([tsp_bin, "--version"]),
        },
        "checks": checks,
        "applicability": {
            "projectionSqlAndTypes": {
                "state": "applicable",
                "reason": (
                    "TypeSpec and JSON Schema/OpenAPI independently project the "
                    "idempotency model into SQL and client-type witnesses."
                ),
                "owner": "ORESoftware/ores-middleware#5",
            },
            "dieselSeaOrmProjectionWitness": {
                "state": "applicable",
                "reason": (
                    "TypeSpec and JSON Schema/OpenAPI each emit compile-checked "
                    "Diesel and SeaORM witnesses; all four manifests and "
                    "normalized persistence semantics are compared."
                ),
                "owner": "ORESoftware/ores-middleware#5",
            },
            "livePostgresCatalogAndOrmIntrospection": {
                "state": "external_gate",
                "reason": (
                    "Real Diesel and SeaORM compilation, independent SQL "
                    "application, pg_catalog read-back, and four-way row-level "
                    "insert/read/rejection convergence remain mandatory before "
                    "persistence artifacts are admitted."
                ),
                "owner": "ORESoftware/ores-middleware#5",
            },
            "protobufGrpcWireClients": {
                "state": "external_gate",
                "reason": (
                    "The TypeSpec lane reserves Protobuf/gRPC/wire-client output; "
                    "implementing and admitting those generators remains tracked."
                ),
                "owner": "ORESoftware/ores-middleware#3",
            },
            "allLanguageRuntimeDescriptors": {
                "state": "applicable",
                "reason": (
                    "Contract-conformance compiles and runtime-checks Rust, "
                    "TypeScript, Go, Gleam, Elixir, and Erlang descriptors."
                ),
                "owner": "ORESoftware/ores-middleware#3",
            },
        },
        "discrepancies": discrepancies,
        "status": status,
        "zeroUnexplainedFindings": status == "passed",
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--receipt",
        type=Path,
        default=ROOT / "target/audit/receipt.json",
    )
    args = parser.parse_args()
    output = args.receipt if args.receipt.is_absolute() else ROOT / args.receipt
    started = now()
    checks: list[dict[str, Any]] = []
    discrepancies: list[dict[str, Any]] = []

    try:
        findings = run_contract_parity(ROOT)
        discrepancies.extend(normalize_discrepancy(item) for item in findings)
        checks.append(
            {
                "id": "docs-serving-typespec-json-schema-peer-parity",
                "required": True,
                "state": "failed" if findings else "executed",
                "detail": (
                    f"{len(findings)} unexplained discrepancy(s)"
                    if findings
                    else (
                        "enum, property, requiredness, normalized type, and "
                        "authority topology parity passed"
                    )
                ),
            }
        )
    except Exception as exc:
        checks.append(
            {
                "id": "docs-serving-typespec-json-schema-peer-parity",
                "required": True,
                "state": "failed",
                "detail": f"{type(exc).__name__}: {exc}",
            }
        )

    try:
        findings, projection_report = run_schema_convergence(
            ROOT, ROOT / "target/schema-convergence"
        )
        discrepancies.extend(normalize_discrepancy(item) for item in findings)
        checks.append(
            {
                "id": "sql-type-diesel-seaorm-projection-convergence",
                "required": True,
                "state": "failed" if findings else "executed",
                "detail": (
                    f"{len(findings)} unexplained discrepancy(s)"
                    if findings
                    else json.dumps(
                        projection_report["compileWitnesses"], sort_keys=True
                    )
                ),
            }
        )
    except Exception as exc:
        checks.append(
            {
                "id": "sql-type-diesel-seaorm-projection-convergence",
                "required": True,
                "state": "failed",
                "detail": f"{type(exc).__name__}: {exc}",
            }
        )

    checks.append(internal("json-schema-meta-validation", validate_schemas))
    checks.append(internal("zpkg-polyglot-manifest", validate_zpkg_contract))
    checks.append(
        run_command(
            "peer-parity-negative-tests",
            [
                sys.executable,
                "-m",
                "unittest",
                "scripts/test_contract_parity.py",
                "-v",
            ],
        )
    )
    checks.append(
        run_command(
            "projection-negative-tests",
            [
                sys.executable,
                "-m",
                "unittest",
                "scripts/test_schema_convergence.py",
                "-v",
            ],
        )
    )

    tsp_bin = os.environ.get("TSP_BIN", "tsp")
    checks.append(
        run_command(
            "typespec-docs-serving-compile",
            [tsp_bin, "compile", "contracts/docs-serving.tsp", "--no-emit"],
        )
    )
    checks.append(
        run_command(
            "typespec-persistence-compile",
            [
                tsp_bin,
                "compile",
                "contracts/persistence/idempotency-record.tsp",
                "--no-emit",
            ],
        )
    )
    checks.append(
        run_command("workspace-contract-check", ["npm", "run", "contracts:check"])
    )
    checks.append(
        run_command(
            "docs-serving-json-schema-fixture-validation",
            ["node", "scripts/validate-docs-serving.mjs"],
        )
    )
    checks.append(
        run_command(
            "typescript-sdk-and-docs-serving-conformance",
            ["npm", "test", "--prefix", "src/ts"],
        )
    )
    checks.append(
        run_command(
            "golang-sdk-and-docs-serving-conformance",
            ["go", "test", "./..."],
            cwd=ROOT / "src/golang",
        )
    )
    checks.append(
        run_command(
            "rust-sdk-and-docs-serving-conformance",
            [
                "cargo",
                "test",
                "--manifest-path",
                "src/rust/Cargo.toml",
                "--all-features",
            ],
            timeout=600,
        )
    )
    checks.append(
        run_command(
            "rust-rollout-transform-tests",
            [sys.executable, "scripts/test-rollout-rust-server.py"],
        )
    )
    checks.append(
        run_command(
            "compiled-targets-rust-ts-go",
            [
                sys.executable,
                "scripts/build_targets.py",
                "--languages",
                "rust,ts,golang",
            ],
            timeout=900,
        )
    )

    draft = receipt(started, checks, discrepancies)
    try:
        schema = json.loads(RECEIPT_SCHEMA.read_text(encoding="utf-8"))
        Draft202012Validator(
            schema, format_checker=FormatChecker()
        ).validate(draft)
        checks.append(
            {
                "id": "audit-receipt-schema-validation",
                "required": True,
                "state": "executed",
                "detail": "receipt validates as ores.schema-audit-receipt/v1",
            }
        )
    except Exception as exc:
        checks.append(
            {
                "id": "audit-receipt-schema-validation",
                "required": True,
                "state": "failed",
                "detail": f"{type(exc).__name__}: {exc}",
            }
        )

    final = receipt(started, checks, discrepancies)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(
        json.dumps(final, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(f"audit status={final['status']} receipt={output}")
    if final["status"] == "passed":
        return 0
    if final["status"] == "stopped_for_evaluation":
        return 2
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
