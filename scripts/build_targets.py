#!/usr/bin/env python3
"""Build each first-class language package below target/<language>.

The orchestrator never treats generated output as a contract authority. It can
build selected languages for CI jobs with different toolchains, while the
zero-argument form builds all six zed-pkg targets.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / "target"
LANGUAGES = ("rust", "ts", "golang", "gleam", "elixir", "erlang")


def run(command: list[str], *, cwd: Path = ROOT, env: dict[str, str] | None = None) -> None:
    merged = os.environ.copy()
    if env:
        merged.update(env)
    subprocess.run(command, cwd=cwd, env=merged, check=True)


def reset_output(language: str) -> Path:
    output = TARGET / language
    if output.exists():
        shutil.rmtree(output)
    output.mkdir(parents=True)
    return output


def copy_if_present(source: Path, destination: Path) -> None:
    if source.exists():
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)


def build_rust() -> None:
    output = reset_output("rust")
    run([
        "cargo", "build", "--manifest-path", "src/rust/Cargo.toml", "--all-features",
        "--target-dir", str(output / "sdk"),
    ])
    run([
        "cargo", "build", "--manifest-path", "src/rust/docs-serving/Cargo.toml",
        "--target-dir", str(output / "docs-serving"),
    ])


def build_ts() -> None:
    output = reset_output("ts")
    run(["npm", "run", "build", "--prefix", "src/ts"])
    shutil.copytree(ROOT / "src/ts/dist", output / "sdk/dist")
    copy_if_present(ROOT / "src/ts/package.json", output / "sdk/package.json")
    shutil.copytree(
        ROOT / "src/ts/docs-serving",
        output / "docs-serving",
        ignore=shutil.ignore_patterns("node_modules", "*.log"),
    )


def build_golang() -> None:
    output = reset_output("golang")
    source = ROOT / "src/golang"
    run(["go", "test", "./..."], cwd=source)
    run(["go", "test", "-c", "-o", str(output / "oresmiddleware.test"), "."], cwd=source)
    run(["go", "test", "-c", "-o", str(output / "docsserving.test"), "./docsserving"], cwd=source)
    copy_if_present(source / "go.mod", output / "go.mod")
    copy_if_present(source / "go.sum", output / "go.sum")


def build_gleam() -> None:
    output = reset_output("gleam")
    source = ROOT / "src/gleam"
    run(["gleam", "build", "--target", "erlang"], cwd=source)
    shutil.copytree(source / "build", output / "build")
    copy_if_present(source / "gleam.toml", output / "gleam.toml")
    copy_if_present(source / "manifest.toml", output / "manifest.toml")


def build_elixir() -> None:
    output = reset_output("elixir")
    source = ROOT / "src/elixir"
    run(
        ["mix", "compile", "--warnings-as-errors"],
        cwd=source,
        env={
            "MIX_BUILD_PATH": str(output / "_build"),
            "MIX_DEPS_PATH": str(source / "deps"),
        },
    )
    copy_if_present(source / "mix.exs", output / "mix.exs")
    copy_if_present(source / "mix.lock", output / "mix.lock")


def build_erlang() -> None:
    output = reset_output("erlang")
    source = ROOT / "src/erlang"
    run(["rebar3", "compile"], cwd=source, env={"REBAR_BASE_DIR": str(output / "_build")})
    copy_if_present(source / "rebar.config", output / "rebar.config")
    copy_if_present(source / "rebar.lock", output / "rebar.lock")


BUILDERS = {
    "rust": build_rust,
    "ts": build_ts,
    "golang": build_golang,
    "gleam": build_gleam,
    "elixir": build_elixir,
    "erlang": build_erlang,
}


def parse_languages(value: str) -> tuple[str, ...]:
    if value == "all":
        return LANGUAGES
    selected = tuple(item.strip() for item in value.split(",") if item.strip())
    unknown = sorted(set(selected) - set(LANGUAGES))
    if unknown:
        raise ValueError(f"unknown language target(s): {', '.join(unknown)}")
    if not selected:
        raise ValueError("at least one language target is required")
    return selected


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--languages",
        default="all",
        help="Comma-separated subset of rust,ts,golang,gleam,elixir,erlang; default: all",
    )
    args = parser.parse_args()
    try:
        selected = parse_languages(args.languages)
    except ValueError as exc:
        parser.error(str(exc))
    for language in selected:
        BUILDERS[language]()
        print(f"built target/{language}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
