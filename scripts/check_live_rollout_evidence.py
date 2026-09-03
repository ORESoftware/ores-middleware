#!/usr/bin/env python3
"""Validate the static current-default-branch rollout evidence receipt.

The receipt is reviewable Git evidence, not a live GitHub API assertion and not
an API/schema authority. Cross-organization private repository reads require
credentials that are deliberately not part of central CI. Each row therefore
anchors a runtime merge, a documentation-evidence merge, and the exact head
whose repository workflow passed.
"""

from __future__ import annotations

import argparse
import json
import re
from collections import Counter
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_RECEIPT = ROOT / "rollout/live-adoptions.json"
SHA = re.compile(r"^[0-9a-f]{40}$")


def _positive_int(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool) and value > 0


def validate(document: Any) -> list[str]:
    errors: list[str] = []
    if not isinstance(document, dict):
        return ["receipt must be a JSON object"]

    if document.get("schema") != "ores.middleware.live-rollout/v1":
        errors.append("schema must be ores.middleware.live-rollout/v1")
    if document.get("evidenceClass") != "current-default-branch":
        errors.append("evidenceClass must be current-default-branch")
    if document.get("deploymentState") != "not-asserted":
        errors.append("deploymentState must remain not-asserted")

    expected_defaults = {
        "adapter": "axum",
        "conclusion": "success",
        "defaultBranch": "main",
        "documentationPath": "docs/ores-middleware.md",
        "manifestPath": "Cargo.toml",
        "temporaryRolloutControls": [],
    }
    if document.get("defaults") != expected_defaults:
        errors.append("defaults drifted from the reviewed live-adoption criteria")

    minimums = document.get("minimums")
    if not isinstance(minimums, dict):
        errors.append("minimums must be an object")
        minimums = {}
    minimum_repositories = minimums.get("repositories")
    minimum_organizations = minimums.get("organizations")
    if not _positive_int(minimum_repositories):
        errors.append("minimums.repositories must be a positive integer")
        minimum_repositories = 20
    if not _positive_int(minimum_organizations):
        errors.append("minimums.organizations must be a positive integer")
        minimum_organizations = 6

    repositories = document.get("repositories")
    if not isinstance(repositories, list):
        errors.append("repositories must be an array")
        repositories = []

    names: list[str] = []
    organizations: list[str] = []
    adapters: Counter[str] = Counter()
    evidence_merges: list[str] = []
    for index, row in enumerate(repositories):
        prefix = f"repositories[{index}]"
        if not isinstance(row, dict):
            errors.append(f"{prefix} must be an object")
            continue

        repository = row.get("repository")
        if not isinstance(repository, str) or repository.count("/") != 1:
            errors.append(f"{prefix}.repository must be owner/name")
        else:
            names.append(repository)
            organizations.append(repository.split("/", 1)[0])

        source = row.get("sourcePath")
        if not isinstance(source, str) or not source.startswith("src/") or source.endswith("/"):
            errors.append(f"{prefix}.sourcePath must name a file below src/")

        revision = row.get("middlewareRevision")
        if not isinstance(revision, str) or SHA.fullmatch(revision) is None:
            errors.append(f"{prefix}.middlewareRevision must be a full lowercase commit SHA")

        runtime = row.get("runtime")
        if not isinstance(runtime, dict):
            errors.append(f"{prefix}.runtime must be an object")
        else:
            if not _positive_int(runtime.get("pr")):
                errors.append(f"{prefix}.runtime.pr must be a positive integer")
            if not isinstance(runtime.get("merge"), str) or SHA.fullmatch(runtime["merge"]) is None:
                errors.append(f"{prefix}.runtime.merge must be a full lowercase commit SHA")

        evidence = row.get("evidence")
        if not isinstance(evidence, dict):
            errors.append(f"{prefix}.evidence must be an object")
        else:
            if not _positive_int(evidence.get("pr")):
                errors.append(f"{prefix}.evidence.pr must be a positive integer")
            for field in ("head", "merge"):
                value = evidence.get(field)
                if not isinstance(value, str) or SHA.fullmatch(value) is None:
                    errors.append(f"{prefix}.evidence.{field} must be a full lowercase commit SHA")
            workflow = evidence.get("workflow")
            if not isinstance(workflow, str) or not workflow.strip():
                errors.append(f"{prefix}.evidence.workflow must be non-empty")
            merge = evidence.get("merge")
            if isinstance(merge, str):
                evidence_merges.append(merge)

        adapters[expected_defaults["adapter"]] += 1

    duplicate_names = sorted(name for name, count in Counter(names).items() if count > 1)
    if duplicate_names:
        errors.append(f"duplicate repositories: {duplicate_names}")
    duplicate_merges = sorted(
        merge for merge, count in Counter(evidence_merges).items() if count > 1
    )
    if duplicate_merges:
        errors.append(f"duplicate evidence merge commits: {duplicate_merges}")

    repository_count = len(repositories)
    organization_count = len(set(organizations))
    if repository_count < minimum_repositories:
        errors.append(
            f"verified repositories below minimum: {repository_count} < {minimum_repositories}"
        )
    if organization_count < minimum_organizations:
        errors.append(
            f"verified organizations below minimum: {organization_count} < {minimum_organizations}"
        )

    computed_summary = {
        "adapterFamilies": dict(sorted(adapters.items())),
        "organizations": organization_count,
        "repositories": repository_count,
    }
    if document.get("summary") != computed_summary:
        errors.append("summary does not match computed repositories, organizations, and adapters")
    return errors


def load(path: Path = DEFAULT_RECEIPT) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--receipt", type=Path, default=DEFAULT_RECEIPT)
    args = parser.parse_args()
    try:
        document = load(args.receipt)
    except (OSError, json.JSONDecodeError) as exc:
        print(f"invalid live rollout receipt: {exc}")
        return 1

    errors = validate(document)
    if errors:
        print("invalid live rollout receipt:")
        for error in errors:
            print(f"- {error}")
        return 1

    print(
        "live rollout evidence passed: "
        f"{document['summary']['repositories']} repositories across "
        f"{document['summary']['organizations']} organizations; "
        "deployment remains not asserted"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
