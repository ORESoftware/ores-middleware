#!/usr/bin/env python3
"""Compatibility shim for the typed Rust Zed repository-contract authority."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import tempfile
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "tools" / "contract-parity" / "Cargo.toml"
BINARY = "zpkg_contract_check"


def _command(root: Path, receipt: Path) -> list[str]:
    return [
        "cargo",
        "run",
        "--quiet",
        "--manifest-path",
        str(MANIFEST),
        "--bin",
        BINARY,
        "--",
        "--root",
        str(root),
        "--receipt",
        str(receipt),
    ]


def _run(root: Path, receipt: Path) -> subprocess.CompletedProcess[str]:
    environment = os.environ.copy()
    environment.setdefault(
        "CARGO_TARGET_DIR",
        str(root / "target" / "repository-control" / "zpkg-contract-check"),
    )
    return subprocess.run(
        _command(root, receipt),
        cwd=ROOT,
        env=environment,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def _read_receipt(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        return {
            "status": "failed",
            "findings": [
                {
                    "code": "rust_receipt_unavailable",
                    "path": str(path),
                    "detail": str(error),
                }
            ],
        }
    return value if isinstance(value, dict) else {
        "status": "failed",
        "findings": [
            {
                "code": "rust_receipt_invalid",
                "path": str(path),
                "detail": "Rust checker receipt must be a JSON object",
            }
        ],
    }


def _finding_text(item: Any) -> str:
    if not isinstance(item, dict):
        return f"invalid Rust finding: {item!r}"
    code = item.get("code", "unknown")
    path = item.get("path", ".zpkg.toml")
    detail = item.get("detail", "missing detail")
    return f"{code} {path}: {detail}"


def validate(root: Path) -> list[str]:
    root = root.resolve()
    with tempfile.TemporaryDirectory(prefix="ores-zpkg-check-") as temporary:
        receipt = Path(temporary) / "receipt.json"
        try:
            completed = _run(root, receipt)
        except OSError as error:
            return [f"unable to execute typed Rust zpkg checker: {error}"]
        report = _read_receipt(receipt)
        findings = [_finding_text(item) for item in report.get("findings", [])]
        if completed.returncode == 0 and report.get("status") == "passed" and not findings:
            return []
        if completed.returncode not in {0, 2}:
            output = (completed.stderr or completed.stdout).strip()
            findings.append(
                f"typed Rust zpkg checker exited {completed.returncode}: {output or 'no output'}"
            )
        if not findings:
            findings.append(
                f"typed Rust zpkg checker returned status {report.get('status')!r}"
            )
        return findings


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--receipt", type=Path)
    args = parser.parse_args()
    root = args.root.resolve()

    if args.receipt is None:
        with tempfile.TemporaryDirectory(prefix="ores-zpkg-check-") as temporary:
            receipt = Path(temporary) / "receipt.json"
            try:
                completed = _run(root, receipt)
            except OSError as error:
                print(f"unable to execute typed Rust zpkg checker: {error}")
                return 1
            report = _read_receipt(receipt)
    else:
        receipt = args.receipt.resolve()
        try:
            completed = _run(root, receipt)
        except OSError as error:
            print(f"unable to execute typed Rust zpkg checker: {error}")
            return 1
        report = _read_receipt(receipt)

    findings = [_finding_text(item) for item in report.get("findings", [])]
    if completed.returncode == 0 and report.get("status") == "passed" and not findings:
        print(
            ".zpkg.toml polyglot contract passed through the typed Rust authority: "
            "repository + rust + typescript + golang + gleam + elixir + erlang + "
            "installed closure + peer-authority gates"
        )
        return 0

    print("invalid .zpkg.toml:")
    for item in findings:
        print(f"- {item}")
    if completed.returncode not in {0, 2}:
        output = (completed.stderr or completed.stdout).strip()
        print(
            f"- typed Rust zpkg checker exited {completed.returncode}: "
            f"{output or 'no output'}"
        )
        return 1
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
