#!/usr/bin/env python3
"""Run the standard Rust rollout helper and select the Axum 0.7 adapter."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path


def argument(name: str) -> str:
    try:
        return sys.argv[sys.argv.index(name) + 1]
    except (ValueError, IndexError) as error:
        raise SystemExit(f"missing required argument {name}") from error


def main() -> int:
    root = Path(__file__).resolve().parent
    subprocess.run(
        [sys.executable, str(root / "rollout-rust-server.py"), *sys.argv[1:]],
        check=True,
    )

    source = Path(argument("--source"))
    manifest = Path(argument("--manifest"))

    source_text = source.read_text()
    marker = "ores_middleware::frameworks::axum::install_from_env"
    replacement = "ores_middleware::frameworks::axum07::install_from_env"
    if marker not in source_text:
        raise SystemExit(f"standard Axum adapter marker not found in {source}")
    source.write_text(source_text.replace(marker, replacement, 1))

    manifest_text = manifest.read_text()
    feature_marker = 'features = ["axum"]'
    if feature_marker not in manifest_text:
        raise SystemExit(f"standard Axum dependency feature not found in {manifest}")
    manifest.write_text(
        manifest_text.replace(feature_marker, 'features = ["axum07"]', 1)
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
