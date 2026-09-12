#!/usr/bin/env python3
"""Audit verified ores-middleware adoption across public Rust server branches.

A branch is verified only when the live source is wrapped, the manifest pins the
central repository by immutable revision, adoption documentation exists, and no
temporary rollout workflow remains. Implementation commits are created only after
the reusable caller's format and cargo-check gate succeeds.
"""

from __future__ import annotations

import argparse
import json
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import asdict, dataclass
from pathlib import Path

BRANCH = "feat/ores-middleware-v1"
CANDIDATES = (
    ("3FA-app/3fa-admin-api-server.rs", "src/server.rs", "Cargo.toml"),
    ("3FA-app/3fa-api-server.rs", "src/server.rs", "Cargo.toml"),
    ("athlet-o/athleto-api-server.rs", "src/main.rs", "Cargo.toml"),
    ("benefactor-cc/benefactor-api-server.rs", "src/application.rs", "Cargo.toml"),
    ("claritas-viz/claritas-api-server.rs", "src/server.rs", "Cargo.toml"),
    ("messaging-intel/msgint-api-server.rs", "src/server.rs", "Cargo.toml"),
    ("messaging-intel/msgint-admin-api-server.rs", "src/server.rs", "Cargo.toml"),
    ("fiducia-cloud/fiducia-api-server.rs", "src/server.rs", "Cargo.toml"),
    ("fiducia-cloud/fiducia-admin-api-server.rs", "src/server.rs", "Cargo.toml"),
    ("opto-sync/opto-sync-api-server.rs", "src/server.rs", "Cargo.toml"),
    ("opto-sync/opto-sync-admin-api-server.rs", "src/server.rs", "Cargo.toml"),
    ("scintilla-run/scintilla-api-server.rs", "src/main.rs", "Cargo.toml"),
    ("quaestor-ledger/quaestor-api-server.rs", "src/server.rs", "Cargo.toml"),
    ("quaestor-ledger/quaestor-admin-api-server.rs", "src/server.rs", "Cargo.toml"),
    ("zed-pkg/zed-api-server.rs", "src/server.rs", "Cargo.toml"),
    ("zed-pkg/zed-admin-api-server.rs", "src/server.rs", "Cargo.toml"),
    ("sonus-auris/sonus-auris-api-server.rs", "src/service.rs", "Cargo.toml"),
    ("sonus-auris/sonus-auris-admin-api-server.rs", "src/server.rs", "Cargo.toml"),
    ("voxletra/vxl-api-server.rs", "crates/api/src/server.rs", "crates/api/Cargo.toml"),
    ("hypesiege/hypesiege-api-server.rs", "src/server.rs", "Cargo.toml"),
    ("memebank/memebank-api-server.rs", "src/main.rs", "Cargo.toml"),
    ("usa-acc/usa-acc-api-server.rs", "src/main.rs", "Cargo.toml"),
    ("flags-2-env/flags-2-env-api-server.rs", "src/server.rs", "Cargo.toml"),
    ("happy-wakey/happy-wakey-api-server.rs", "src/main.rs", "Cargo.toml"),
    ("chapter-publishing/cp-api-server.rs", "src/main.rs", "Cargo.toml"),
    ("premarital-asset-protection/pmap-api-server.rs", "src/main.rs", "Cargo.toml"),
    ("evento-globolo/evgl-api-server.rs", "src/main.rs", "Cargo.toml"),
)
WORKFLOW_PATHS = (
    ".github/workflows/ores-middleware-rollout.yml",
    ".github/workflows/ores-middleware-rollout-retry.yml",
)


@dataclass(frozen=True)
class Result:
    repository: str
    source: str
    manifest: str
    source_marker: bool
    immutable_pin: bool
    documentation: bool
    controls_removed: bool
    adapter: str
    verified: bool


def fetch(repository: str, path: str) -> tuple[int, str]:
    encoded_path = "/".join(urllib.parse.quote(part, safe="") for part in path.split("/"))
    url = f"https://raw.githubusercontent.com/{repository}/{BRANCH}/{encoded_path}"
    request = urllib.request.Request(url, headers={"User-Agent": "ores-middleware-rollout-audit/1"})
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            return response.status, response.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as error:
        return error.code, ""
    except (TimeoutError, urllib.error.URLError):
        return 0, ""


def audit(repository: str, source: str, manifest: str) -> Result:
    source_status, source_text = fetch(repository, source)
    manifest_status, manifest_text = fetch(repository, manifest)
    docs_status, _ = fetch(repository, "docs/ores-middleware.md")
    workflow_statuses = [fetch(repository, path)[0] for path in WORKFLOW_PATHS]

    source_marker = source_status == 200 and (
        "ores_middleware::frameworks::axum::install_from_env" in source_text
        or "ores_middleware::frameworks::axum07::install_from_env" in source_text
    )
    immutable_pin = manifest_status == 200 and all(
        token in manifest_text
        for token in (
            'git = "https://github.com/ORESoftware/ores-middleware"',
            "rev = ",
        )
    )
    documentation = docs_status == 200
    controls_removed = all(status == 404 for status in workflow_statuses)
    adapter = (
        "axum07"
        if "frameworks::axum07::install_from_env" in source_text
        else "axum"
        if "frameworks::axum::install_from_env" in source_text
        else ""
    )
    verified = source_marker and immutable_pin and documentation and controls_removed
    return Result(
        repository=repository,
        source=source,
        manifest=manifest,
        source_marker=source_marker,
        immutable_pin=immutable_pin,
        documentation=documentation,
        controls_removed=controls_removed,
        adapter=adapter,
        verified=verified,
    )


def markdown(results: list[Result]) -> str:
    verified = [result for result in results if result.verified]
    organizations = sorted({result.repository.split("/", 1)[0] for result in verified})
    lines = [
        "# ORES middleware Rust server rollout audit",
        "",
        f"Verified implementations: **{len(verified)} / {len(results)}**",
        f"Organizations represented: **{len(organizations)}**",
        "",
        "A branch counts only when its live source contains the adapter, its manifest pins an immutable central revision, adoption documentation exists, and all temporary rollout workflows are removed. The reusable workflow creates that state only after formatting and `cargo check --all-targets` succeed.",
        "",
        "| Status | Repository | Adapter | Source | Pin | Docs | Controls removed |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    for result in results:
        mark = "✅" if result.verified else "❌"
        yes = lambda value: "yes" if value else "no"
        lines.append(
            f"| {mark} | `{result.repository}` | `{result.adapter or '-'}` | {yes(result.source_marker)} | {yes(result.immutable_pin)} | {yes(result.documentation)} | {yes(result.controls_removed)} |"
        )
    lines.extend(("", "Organizations: " + ", ".join(f"`{name}`" for name in organizations)))
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--minimum", type=int, default=0)
    parser.add_argument("--json", type=Path, default=Path("target/rollout-audit.json"))
    parser.add_argument("--markdown", type=Path, default=Path("target/rollout-audit.md"))
    args = parser.parse_args()

    results: list[Result] = []
    for candidate in CANDIDATES:
        results.append(audit(*candidate))
        time.sleep(0.05)

    args.json.parent.mkdir(parents=True, exist_ok=True)
    args.markdown.parent.mkdir(parents=True, exist_ok=True)
    args.json.write_text(json.dumps([asdict(result) for result in results], indent=2) + "\n")
    args.markdown.write_text(markdown(results))
    print(args.markdown.read_text(), end="")

    count = sum(result.verified for result in results)
    organizations = len({result.repository.split("/", 1)[0] for result in results if result.verified})
    if count < args.minimum:
        print(f"rollout gate failed: {count} verified implementations; minimum is {args.minimum}")
        return 1
    if args.minimum >= 20 and organizations < 6:
        print(f"rollout gate failed: {organizations} verified organizations; minimum is 6")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
