#!/usr/bin/env python3
"""Run the complete repository audit and always write one machine-readable receipt."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shlex
import subprocess
import sys
from dataclasses import asdict
from datetime import datetime, timezone
from importlib import metadata
from pathlib import Path
from typing import Any, Callable

from jsonschema import Draft202012Validator, FormatChecker

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.check_contract_parity import Discrepancy, run as run_contract_parity

RECEIPT_SCHEMA = ROOT / "contracts" / "schema-audit-receipt.schema.json"


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def trim_output(value: str, limit: int = 3000) -> str:
    value = value.strip()
    if not value:
        return "passed"
    if len(value) <= limit:
        return value
    return "…" + value[-limit:]


def command_text(command: list[str]) -> str:
    return shlex.join(command)


def run_command(
    check_id: str,
    command: list[str],
    *,
    cwd: Path = ROOT,
    env: dict[str, str] | None = None,
) -> dict[str, Any]:
    merged_env = os.environ.copy()
    if env:
        merged_env.update(env)
    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            env=merged_env,
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=180,
        )
    except (OSError, subprocess.SubprocessError) as exc:
        return {
            "id": check_id,
            "required": True,
            "state": "failed",
            "command": command_text(command),
            "exitCode": 127,
            "detail": str(exc),
        }
    return {
        "id": check_id,
        "required": True,
        "state": "executed" if completed.returncode == 0 else "failed",
        "command": command_text(command),
        "exitCode": completed.returncode,
        "detail": trim_output(completed.stdout),
    }


def internal_check(check_id: str, fn: Callable[[], str]) -> dict[str, Any]:
    try:
        detail = fn()
    except Exception as exc:  # receipt must survive every checker boundary
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


def validate_authored_json_schemas() -> str:
    checked = []
    for relative in (
        "contracts/docs-serving.schema.json",
        "contracts/schema-audit-receipt.schema.json",
    ):
        path = ROOT / relative
        schema = json.loads(path.read_text(encoding="utf-8"))
        Draft202012Validator.check_schema(schema)
        checked.append(relative)
    return "validated Draft 2020-12 schemas: " + ", ".join(checked)


def validate_ci_pins() -> str:
    failures = []
    workflows = sorted((ROOT / ".github" / "workflows").glob("*.y*ml"))
    for path in workflows:
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            match = re.match(r"^\s*uses:\s*([^\s#]+)", line)
            if match:
                reference = match.group(1)
                if not reference.startswith("./") and not re.fullmatch(
                    r"[^@]+@[0-9a-fA-F]{40}", reference
                ):
                    failures.append(f"{path.relative_to(ROOT)}:{number}: mutable action {reference}")
            if re.match(r"^\s*runs-on:\s*ubuntu-latest\s*$", line):
                failures.append(f"{path.relative_to(ROOT)}:{number}: mutable runner label")
    if failures:
        raise ValueError("; ".join(failures))
    return f"checked {len(workflows)} workflow(s) for immutable action and runner references"


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
    text = completed.stdout.strip().splitlines()
    return text[0] if completed.returncode == 0 and text else fallback


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
    included_roots = [
        ROOT / ".github",
        ROOT / "contracts",
        ROOT / "docs",
        ROOT / "fixtures",
        ROOT / "scripts",
        ROOT / "src",
    ]
    files = []
    for base in included_roots:
        if not base.exists():
            continue
        for path in base.rglob("*"):
            if not path.is_file():
                continue
            if any(part in {"node_modules", "target", "__pycache__"} for part in path.parts):
                continue
            files.append(path.relative_to(ROOT).as_posix())
    files.extend(["AGENTS.md", "README.md", ".gitignore"])
    return sorted(set(item for item in files if (ROOT / item).is_file()))


def build_receipt(
    *,
    started_at: str,
    ended_at: str,
    checks: list[dict[str, Any]],
    discrepancies: list[Discrepancy],
) -> dict[str, Any]:
    has_required_failure = any(
        check["required"] and check["state"] != "executed" for check in checks
    )
    if discrepancies:
        status = "stopped_for_evaluation"
    elif has_required_failure:
        status = "failed"
    else:
        status = "passed"

    files = source_files()
    contract_sources = [
        "contracts/docs-serving.tsp",
        "contracts/docs-serving.schema.json",
        "contracts/authority-topology.json",
        "fixtures/docs-serving-conformance.tsv",
    ]
    tsp_bin = os.environ.get("TSP_BIN", "tsp")
    tools = {
        "python": sys.version.split()[0],
        "jsonschema": metadata.version("jsonschema"),
        "node": tool_version(["node", "--version"]),
        "go": tool_version(["go", "version"]),
        "cargo": tool_version(["cargo", "--version"]),
        "rustfmt": tool_version(["cargo", "fmt", "--version"]),
        "typespec": tool_version([tsp_bin, "--version"]),
    }
    return {
        "schema": "ores.schema-audit-receipt/v1",
        "repository": "ORESoftware/ores-middleware",
        "startedAt": started_at,
        "endedAt": ended_at,
        "actor": os.environ.get("GITHUB_ACTOR") or os.environ.get("USER") or "unknown",
        "scope": {
            "commit": current_commit(),
            "files": files,
            "sourceDigests": {
                relative: digest(ROOT / relative) for relative in contract_sources
            },
            "exclusions": [
                ".git internals",
                "target generated output",
                "node_modules and language build caches",
                "credential values and unreviewed external systems",
            ],
        },
        "tools": tools,
        "checks": checks,
        "applicability": {
            "apiDocsArtifactGeneration": {
                "state": "external_gate",
                "reason": "OpenAPI/OpenRPC/Connect/Hyper-Schema/catalog bytes and RPC digests are produced by ORESoftware/api-docs, not this selector.",
                "owner": "ORESoftware/api-docs#19",
            },
            "sqlCatalogParity": {
                "state": "external_gate",
                "reason": "The selector is stateless; SQL_T and SQL_J catalog read-back is mandatory in each database-backed *-lib-core before artifacts reach middleware.",
                "owner": "Linear DEN-3321",
            },
            "dieselSeaOrmParity": {
                "state": "external_gate",
                "reason": "No ORM model exists here; independent Diesel/SeaORM/catalog convergence remains a release gate for upstream persistence contracts.",
                "owner": "Linear DEN-3321",
            },
        },
        "discrepancies": [asdict(item) for item in discrepancies],
        "status": status,
        "zeroUnexplainedFindings": status == "passed",
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--receipt",
        type=Path,
        default=ROOT / "target" / "audit" / "receipt.json",
    )
    args = parser.parse_args()
    receipt_path = args.receipt
    if not receipt_path.is_absolute():
        receipt_path = ROOT / receipt_path

    started_at = utc_now()
    checks: list[dict[str, Any]] = []
    discrepancies: list[Discrepancy] = []

    try:
        discrepancies = run_contract_parity(ROOT)
        checks.append(
            {
                "id": "typespec-json-schema-peer-parity",
                "required": True,
                "state": "failed" if discrepancies else "executed",
                "detail": (
                    f"{len(discrepancies)} unexplained discrepancy(s)"
                    if discrepancies
                    else "enum values, model properties, requiredness, normalized types, and peer topology match"
                ),
            }
        )
    except Exception as exc:
        checks.append(
            {
                "id": "typespec-json-schema-peer-parity",
                "required": True,
                "state": "failed",
                "detail": f"{type(exc).__name__}: {exc}",
            }
        )

    checks.append(internal_check("json-schema-meta-validation", validate_authored_json_schemas))
    checks.append(internal_check("ci-immutable-pins", validate_ci_pins))
    checks.append(
        run_command(
            "contract-parity-negative-tests",
            [sys.executable, "-m", "unittest", "scripts/test_contract_parity.py", "-v"],
        )
    )

    tsp_bin = os.environ.get("TSP_BIN", "tsp")
    checks.append(
        run_command(
            "typespec-compile",
            [tsp_bin, "compile", "contracts/docs-serving.tsp", "--no-emit"],
        )
    )
    checks.append(run_command("typescript-conformance", ["npm", "test", "--prefix", "src/ts"]))
    checks.append(
        run_command("golang-conformance", ["go", "test", "./..."], cwd=ROOT / "src" / "golang")
    )
    checks.append(
        run_command(
            "rust-format",
            ["cargo", "fmt", "--manifest-path", "src/rust/Cargo.toml", "--check"],
        )
    )
    checks.append(
        run_command(
            "rust-conformance",
            [
                "cargo",
                "test",
                "--manifest-path",
                "src/rust/Cargo.toml",
                "--locked",
                "--target-dir",
                "target/rust",
            ],
        )
    )
    checks.append(
        run_command(
            "language-target-builds",
            [sys.executable, "scripts/build_targets.py"],
        )
    )

    receipt = build_receipt(
        started_at=started_at,
        ended_at=utc_now(),
        checks=checks,
        discrepancies=discrepancies,
    )

    try:
        receipt_schema = json.loads(RECEIPT_SCHEMA.read_text(encoding="utf-8"))
        Draft202012Validator(
            receipt_schema, format_checker=FormatChecker()
        ).validate(receipt)
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

    receipt = build_receipt(
        started_at=started_at,
        ended_at=utc_now(),
        checks=checks,
        discrepancies=discrepancies,
    )
    receipt_path.parent.mkdir(parents=True, exist_ok=True)
    receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    print(f"audit status={receipt['status']} receipt={receipt_path}")
    if receipt["status"] == "passed":
        return 0
    if receipt["status"] == "stopped_for_evaluation":
        return 2
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
