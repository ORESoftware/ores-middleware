#!/usr/bin/env python3
"""Build each first-class language package below target/<language>.

The orchestrator never treats generated output as a contract authority. It can
build selected languages for CI jobs with different toolchains, while the
zero-argument form builds all six zed-pkg targets plus a self-contained
installed-package evidence closure.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / "target"
LANGUAGES = ("rust", "ts", "golang", "gleam", "elixir", "erlang")
PACKAGE_EVIDENCE_INPUTS = (
    Path("contracts/authority-topology.json"),
    Path("contracts/persistence/idempotency-record.tsp"),
    Path("contracts/persistence/idempotency-record.schema.json"),
    Path("scripts/cross_translate.py"),
    Path("scripts/installed_package_smoke.py"),
)


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


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def assert_output_is_self_contained(output: Path) -> None:
    """Reject broken links and links that escape a zed-pkg target root."""

    root = output.resolve(strict=True)
    for path in sorted(output.rglob("*")):
        if not path.is_symlink():
            continue
        relative = path.relative_to(output)
        try:
            resolved = path.resolve(strict=True)
        except FileNotFoundError as exc:
            raise ValueError(f"build output contains broken symlink: {relative}") from exc
        if not resolved.is_relative_to(root):
            raise ValueError(
                "build output symlink escapes target root: "
                f"{relative} -> {resolved} (root {root})"
            )


def materialize_project_directory(
    destination: Path,
    source: Path,
    *,
    required: bool,
) -> None:
    """Replace a rebar3 project link with an in-output directory copy.

    rebar3 intentionally links the current application's ``src``, ``include``,
    and ``priv`` directories back to the checkout. Dependency applications are
    already rooted below ``REBAR_BASE_DIR`` and remain untouched. A declared
    Zed output cannot depend on the checkout path, so project-owned links are
    copied into the output closure; absent optional directories become empty.
    """

    if destination.is_symlink():
        destination.unlink()
    elif destination.exists():
        if not destination.is_dir():
            raise ValueError(f"project output path is not a directory: {destination}")
        return

    if source.is_dir() and not source.is_symlink():
        shutil.copytree(source, destination, symlinks=False)
    elif required:
        raise ValueError(f"required project source directory is missing: {source}")
    else:
        destination.mkdir(parents=True, exist_ok=True)


def materialize_node_runtime_dependencies(project: Path, output: Path) -> None:
    """Copy the lock-defined production dependency closure into a Zed output.

    Zed installs build outputs after the source checkout has disappeared. A
    generated JavaScript module that imports a package must therefore carry the
    exact production packages selected by the native npm lock. Development-only
    packages remain excluded. Missing required or malformed lock entries fail
    closed rather than producing an artifact that works only in the checkout.
    """

    lock_path = project / "package-lock.json"
    document = json.loads(lock_path.read_text(encoding="utf-8"))
    packages = document.get("packages")
    if not isinstance(packages, dict):
        raise ValueError(f"npm lock lacks a packages object: {lock_path}")

    materialized: list[dict[str, str | None]] = []
    for relative, metadata in sorted(packages.items()):
        if not relative.startswith("node_modules/"):
            continue
        if not isinstance(metadata, dict):
            raise ValueError(f"npm lock package metadata is not an object: {relative}")
        if metadaget("dev") is True:
            continue

        source = project / relative
        if not source.exists():
            if metadata.get("optional") is True:
                continue
            raise ValueError(f"required npm runtime dependency is missing: {source}")

        destination = output / relative
        if source.is_dir():
            shutil.copytree(
                source,
                destination,
                symlinks=False,
                dirs_exist_ok=True,
            )
        elif source.is_file():
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, destination)
        else:
            raise ValueError(f"unsupported npm runtime dependency path: {source}")

        materialized.append(
            {
                "path": relative,
                "version": metadata.get("version"),
                "integrity": metadata.get("integrity"),
            }
        )

    package = json.loads((project / "package.json").read_text(encoding="utf-8"))
    declared = package.get("dependencies", {})
    if declared and not materialized:
        raise ValueError(
            "package declares runtime dependencies but the npm lock produced no "
            "materialized production dependency closure"
        )

    (output / "runtime-dependencies.json").write_text(
        json.dumps(
            {
                "schema": "ores.node-runtime-dependency-closure/v1",
                "packageLockSha256": sha256(lock_path),
                "packages": materialized,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )


def build_rust() -> None:
    output = reset_output("rust")
    run([
        "cargo",
        "build",
        "--manifest-path",
        "src/rust/Cargo.toml",
        "--all-features",
        "--target-dir",
        str(output),
    ])


def build_ts() -> None:
    output = reset_output("ts")
    run([
        "npm",
        "ci",
        "--prefix",
        "src/ts",
        "--ignore-scripts",
        "--no-audit",
        "--no-fund",
    ])
    run(["npm", "run", "build", "--prefix", "src/ts"])
    source = ROOT / "src/ts"
    shutil.copytree(source / "dist", output / "dist")
    copy_if_present(source / "package.json", output / "package.json")
    copy_if_present(source / "package-lock.json", output / "package-lock.json")
    materialize_node_runtime_dependencies(source, output)

    # Detect checkout-only module resolution before packaging. The import uses
    # the output file URL, so Node resolves only from target/ts and its copied
    # production dependency closure.
    run(
        [
            "node",
            "--input-type=module",
            "--eval",
            f"await import({json.dumps((output / 'dist/index.js').resolve().as_uri())});",
        ]
    )


def build_golang() -> None:
    output = reset_output("golang")
    source = ROOT / "src/golang"
    run(["go", "test", "./..."], cwd=source)
    run(["go", "build", "-o", str(output / "contractcheck"), "./cmd/contractcheck"], cwd=source)
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
    # Mix creates links from _build/lib/<dependency>/src into MIX_DEPS_PATH.
    # Keeping both roots under target/elixir makes the compiled package
    # self-contained for zed-pkg copy-mode installation.
    environment = {
        "MIX_BUILD_PATH": str(output / "_build"),
        "MIX_DEPS_PATH": str(output / "deps"),
    }
    run(["mix", "deps.get"], cwd=source, env=environment)
    run(["mix", "compile", "--warnings-as-errors"], cwd=source, env=environment)
    copy_if_present(source / "mix.exs", output / "mix.exs")
    copy_if_present(source / "mix.lock", output / "mix.lock")


def build_erlang() -> None:
    output = reset_output("erlang")
    source = ROOT / "src/erlang"
    run(["rebar3", "compile"], cwd=source, env={"REBAR_BASE_DIR": str(output / "_build")})

    application = output / "_build" / "default" / "lib" / "ores_middleware"
    if not application.is_dir() or application.is_symlink():
        raise ValueError(f"compiled Erlang application is missing: {application}")
    materialize_project_directory(application / "src", source / "src", required=True)
    materialize_project_directory(application / "include", source / "include", required=False)
    materialize_project_directory(application / "priv", source / "priv", required=False)

    copy_if_present(source / "rebar.config", output / "rebar.config")
    copy_if_present(source / "rebar.lock", output / "rebar.lock")


def build_package_evidence() -> None:
    """Materialize the contract/tool closure required by installed smoke tests.

    Zed installs declared build outputs, not the full source checkout. Keep the
    two authored persistence contracts and the non-authoritative comparison
    tool in a distinct output; this lets ``r2g`` rerun the comparison after the
    source checkout has disappeared without presenting the copy as an authority.
    """

    output = reset_output("package")
    source_digests: dict[str, str] = {}
    for relative in PACKAGE_EVIDENCE_INPUTS:
        source = ROOT / relative
        if not source.is_file():
            raise ValueError(f"required installed-package evidence is missing: {relative}")
        destination = output / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
        source_digests[relative.as_posix()] = sha256(destination)

    manifest = {
        "schema": "ores.zed-installed-package/v1",
        "authoritative": False,
        "purpose": "installed-package-release-evidence",
        "targets": list(LANGUAGES),
        "sourceDigests": source_digests,
    }
    (output / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


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
        assert_output_is_self_contained(TARGET / language)
        print(f"built target/{language}")
    if set(selected) == set(LANGUAGES):
        build_package_evidence()
        assert_output_is_self_contained(TARGET / "package")
        print("built target/package")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
