#!/usr/bin/env python3
"""Audit verified ores-middleware adoption across Rust server branches.

A server counts only when its live source installs the shared adapter, its Cargo
manifest pins an immutable central commit, adoption documentation exists, and all
temporary rollout controls have been removed. The reusable rollout workflow only
creates that state after formatting and `cargo check --all-targets` succeed.
"""

from __future__ import annotations

import argparse
import base64
import concurrent.futures
import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

BRANCH = "feat/ores-middleware-v1"
CENTRAL_REPOSITORY = "https://github.com/ORESoftware/ores-middleware"
IMMUTABLE_REVISION = re.compile(r"\brev\s*=\s*[\"']([0-9a-fA-F]{40})[\"']")
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
class FetchResult:
    status: int
    text: str


@dataclass(frozen=True)
class AuditResult:
    repository: str
    organization: str
    source: str
    manifest: str
    branch_sha: str
    source_marker: bool
    immutable_pin: bool
    pinned_revision: str
    documentation: bool
    controls_removed: bool
    adapter: str
    verified: bool
    notes: tuple[str, ...]


class GitHubClient:
    def __init__(self, token: str | None) -> None:
        self.token = token

    def request_json(self, url: str) -> tuple[int, Any | None]:
        headers = {
            "Accept": "application/vnd.github+json",
            "User-Agent": "ores-middleware-rollout-audit/2",
            "X-GitHub-Api-Version": "2022-11-28",
        }
        if self.token:
            headers["Authorization"] = f"Bearer {self.token}"
        request = urllib.request.Request(url, headers=headers)
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                return response.status, json.loads(response.read().decode("utf-8"))
        except urllib.error.HTTPError as error:
            payload = error.read().decode("utf-8", "replace")
            try:
                return error.code, json.loads(payload)
            except json.JSONDecodeError:
                return error.code, None
        except (TimeoutError, urllib.error.URLError):
            return 0, None

    def branch_sha(self, repository: str) -> str:
        branch = urllib.parse.quote(BRANCH, safe="")
        status, payload = self.request_json(
            f"https://api.github.com/repos/{repository}/branches/{branch}"
        )
        if status != 200 or not isinstance(payload, dict):
            return ""
        commit = payload.get("commit")
        return str(commit.get("sha", "")) if isinstance(commit, dict) else ""

    def content(self, repository: str, path: str) -> FetchResult:
        encoded_path = "/".join(
            urllib.parse.quote(part, safe="") for part in path.split("/")
        )
        ref = urllib.parse.quote(BRANCH, safe="")
        status, payload = self.request_json(
            f"https://api.github.com/repos/{repository}/contents/{encoded_path}?ref={ref}"
        )
        if status != 200 or not isinstance(payload, dict):
            return FetchResult(status=status, text="")
        encoded = payload.get("content")
        if not isinstance(encoded, str):
            return FetchResult(status=status, text="")
        try:
            text = base64.b64decode(encoded).decode("utf-8", "replace")
        except (ValueError, UnicodeError):
            text = ""
        return FetchResult(status=status, text=text)


def audit_one(client: GitHubClient, candidate: tuple[str, str, str]) -> AuditResult:
    repository, source, manifest = candidate
    source_result = client.content(repository, source)
    manifest_result = client.content(repository, manifest)
    docs_result = client.content(repository, "docs/ores-middleware.md")
    workflow_results = [client.content(repository, path) for path in WORKFLOW_PATHS]
    branch_sha = client.branch_sha(repository)

    source_marker = source_result.status == 200 and (
        "ores_middleware::frameworks::axum::install_from_env" in source_result.text
        or "ores_middleware::frameworks::axum07::install_from_env" in source_result.text
    )
    revision_match = IMMUTABLE_REVISION.search(manifest_result.text)
    pinned_revision = revision_match.group(1).lower() if revision_match else ""
    immutable_pin = (
        manifest_result.status == 200
        and CENTRAL_REPOSITORY in manifest_result.text
        and bool(pinned_revision)
    )
    documentation = docs_result.status == 200 and "Shared request middleware" in docs_result.text
    controls_removed = all(result.status == 404 for result in workflow_results)
    adapter = (
        "axum07"
        if "frameworks::axum07::install_from_env" in source_result.text
        else "axum"
        if "frameworks::axum::install_from_env" in source_result.text
        else ""
    )

    notes: list[str] = []
    if not branch_sha:
        notes.append("branch missing or unreadable")
    if source_result.status != 200:
        notes.append(f"source HTTP {source_result.status}")
    elif not source_marker:
        notes.append("live source lacks adapter marker")
    if manifest_result.status != 200:
        notes.append(f"manifest HTTP {manifest_result.status}")
    elif not immutable_pin:
        notes.append("manifest lacks immutable central pin")
    if not documentation:
        notes.append("adoption documentation missing")
    if not controls_removed:
        notes.append("temporary rollout control remains")

    verified = bool(branch_sha) and source_marker and immutable_pin and documentation and controls_removed
    return AuditResult(
        repository=repository,
        organization=repository.split("/", 1)[0],
        source=source,
        manifest=manifest,
        branch_sha=branch_sha,
        source_marker=source_marker,
        immutable_pin=immutable_pin,
        pinned_revision=pinned_revision,
        documentation=documentation,
        controls_removed=controls_removed,
        adapter=adapter,
        verified=verified,
        notes=tuple(notes),
    )


def markdown(results: list[AuditResult]) -> str:
    verified = [result for result in results if result.verified]
    organizations = sorted({result.organization for result in verified})
    axum07 = sum(result.adapter == "axum07" for result in verified)
    axum08 = sum(result.adapter == "axum" for result in verified)
    lines = [
        "# ORES middleware Rust server rollout audit",
        "",
        f"Verified implementations: **{len(verified)} / {len(results)}**",
        f"Verified organizations: **{len(organizations)}**",
        f"Adapters: **Axum 0.8: {axum08}**, **Axum 0.7: {axum07}**",
        "",
        "A server counts only when its live source installs the adapter, its manifest pins an immutable central revision, adoption documentation exists, and all temporary rollout workflows are removed. The reusable rollout workflow creates that state only after formatting and `cargo check --all-targets` succeed.",
        "",
        "| Status | Repository | Adapter | Branch SHA | Pinned middleware | Notes |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    for result in results:
        mark = "✅" if result.verified else "❌"
        notes = "; ".join(result.notes) or "verified"
        branch_sha = result.branch_sha[:10] if result.branch_sha else "-"
        pinned = result.pinned_revision[:10] if result.pinned_revision else "-"
        lines.append(
            f"| {mark} | `{result.repository}` | `{result.adapter or '-'}` | `{branch_sha}` | `{pinned}` | {notes} |"
        )
    lines.extend(
        (
            "",
            "Verified organizations: "
            + (", ".join(f"`{name}`" for name in organizations) or "none"),
            "",
            "Generated from GitHub branch contents; workflow conclusions alone are not counted.",
        )
    )
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--minimum", type=int, default=0)
    parser.add_argument("--minimum-organizations", type=int, default=0)
    parser.add_argument("--json", type=Path, default=Path("target/rollout-audit.json"))
    parser.add_argument("--markdown", type=Path, default=Path("target/rollout-audit.md"))
    parser.add_argument("--workers", type=int, default=8)
    args = parser.parse_args()

    client = GitHubClient(os.getenv("GITHUB_TOKEN") or os.getenv("GH_TOKEN"))
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as pool:
        results = list(pool.map(lambda candidate: audit_one(client, candidate), CANDIDATES))
    results.sort(key=lambda result: result.repository.lower())

    args.json.parent.mkdir(parents=True, exist_ok=True)
    args.markdown.parent.mkdir(parents=True, exist_ok=True)
    args.json.write_text(json.dumps([asdict(result) for result in results], indent=2) + "\n")
    report = markdown(results)
    args.markdown.write_text(report)
    print(report, end="")

    verified = [result for result in results if result.verified]
    organizations = {result.organization for result in verified}
    failed = False
    if len(verified) < args.minimum:
        print(
            f"rollout gate failed: {len(verified)} verified implementations; minimum is {args.minimum}",
            file=sys.stderr,
        )
        failed = True
    if len(organizations) < args.minimum_organizations:
        print(
            f"rollout gate failed: {len(organizations)} verified organizations; minimum is {args.minimum_organizations}",
            file=sys.stderr,
        )
        failed = True
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
