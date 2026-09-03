#!/usr/bin/env python3
"""Run subprocesses with a clean machine-readable stdout channel."""

from __future__ import annotations

import os
import subprocess
from pathlib import Path


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
        stderr=subprocess.PIPE,
        check=False,
        timeout=timeout,
    )
    stdout = completed.stdout.strip()
    stderr = completed.stderr.strip()
    if completed.returncode:
        detail = "\n".join(part for part in (stdout, stderr) if part)
        raise ValueError(
            f"command failed ({' '.join(command)}): {detail or 'no output'}"
        )
    return stdout
