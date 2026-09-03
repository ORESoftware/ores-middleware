#!/usr/bin/env python3
"""Compatibility bridge to the Rust peer-authority parity checker.

The validation, normalization, hashing, report generation, and negative-test
logic live in ``tools/contract-parity``. This module remains temporarily because
``scripts/audit.py`` imports ``run`` while the wider audit orchestrator is still
Python-based.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "tools" / "contract-parity" / "Cargo.toml"


@dataclass(frozen=True)
class Discrepancy:
    fingerprint: str
    kind: str
    detail: str
    owner: str = "ORESoftware/ores-middleware"
    resolutionState: str = "unexplained"


def _command(root: Path, report: Path) -> list[str]:
    return [
        "cargo",
        "run",
        "--quiet",
        "--manifest-path",
        str(MANIFEST),
        "--",
        "--root",
        str(root),
        "--report",
        str(report),
    ]


def run(root: Path = ROOT) -> list[Discrepancy]:
    with tempfile.TemporaryDirectory(prefix="ores-contract-parity-") as directory:
        report = Path(directory) / "report.json"
        completed = subprocess.run(
            _command(root, report),
            cwd=ROOT,
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )
        if completed.returncode not in (0, 2):
            raise RuntimeError(
                "Rust contract parity checker failed to execute "
                f"(exit {completed.returncode}): {completed.stdout.strip()}"
            )
        if not report.is_file():
            raise RuntimeError("Rust contract parity checker did not write its report")
        document = json.loads(report.read_text(encoding="utf-8"))
        findings = document.get("discrepancies")
        if not isinstance(findings, list):
            raise RuntimeError("Rust contract parity report has no discrepancy list")
        return [Discrepancy(**item) for item in findings]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--report", type=Path)
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args()

    command = _command(args.root, args.report or Path("target/audit/docs-serving-contract-parity.json"))
    return subprocess.run(command, cwd=ROOT, check=False).returncode


if __name__ == "__main__":
    raise SystemExit(main())
