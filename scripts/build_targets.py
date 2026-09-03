#!/usr/bin/env python3
"""Build implemented language packages below target/<language>."""

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / "target"


def run(command: list[str], *, cwd: Path = ROOT) -> None:
    subprocess.run(command, cwd=cwd, check=True)


def build_typescript() -> None:
    output = TARGET / "ts"
    if output.exists():
        shutil.rmtree(output)
    output.mkdir(parents=True)
    shutil.copy2(ROOT / "src" / "ts" / "src" / "index.js", output / "index.js")
    shutil.copy2(ROOT / "src" / "ts" / "src" / "index.d.ts", output / "index.d.ts")
    package = (ROOT / "src" / "ts" / "package.json").read_text(encoding="utf-8")
    (output / "package.json").write_text(package, encoding="utf-8")


def build_go() -> None:
    output = TARGET / "golang"
    output.mkdir(parents=True, exist_ok=True)
    run(
        ["go", "test", "-c", "-o", str(output / "docsserving.test"), "./docsserving"],
        cwd=ROOT / "src" / "golang",
    )


def build_rust() -> None:
    run(
        [
            "cargo",
            "build",
            "--manifest-path",
            "src/rust/Cargo.toml",
            "--locked",
            "--target-dir",
            "target/rust",
        ]
    )


def main() -> int:
    build_typescript()
    build_go()
    build_rust()
    print("built target/ts, target/golang, and target/rust")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
