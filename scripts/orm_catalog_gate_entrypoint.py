#!/usr/bin/env python3
"""Stable entrypoint for the ORM/catalog gate's machine-output subprocesses."""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts import orm_catalog_gate
from scripts.subprocess_capture import run_command

# The implementation's original runner intentionally retained diagnostics during
# development. Production execution requires stdout to contain only witness JSON.
orm_catalog_gate.run_command = run_command


if __name__ == "__main__":
    raise SystemExit(orm_catalog_gate.main())
