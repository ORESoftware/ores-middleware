#!/usr/bin/env python3
"""Static fail-closed checks for middleware Zed release ownership/versioning."""

from __future__ import annotations

import re
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / ".zpkg.toml"
RELEASE_FILES = (
    ROOT / "scripts" / "run-zed-release-acceptance.sh",
    ROOT / ".github" / "workflows" / "zed-source-tag.yml",
    ROOT / ".github" / "workflows" / "zed-release-acceptance.yml",
    ROOT / ".github" / "workflows" / "release-zed.yml",
)


def main() -> int:
    manifest = tomllib.loads(MANIFEST.read_text(encoding="utf-8"))
    package = manifest["package"]
    version = package["version"]
    if re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version) is None:
        raise SystemExit(f"package version is not stable semver: {version!r}")
    if manifest["publish"]["tag_format"] != "v{version}":
        raise SystemExit("publish.tag_format must remain v{version}")

    for path in RELEASE_FILES:
        text = path.read_text(encoding="utf-8")
        if version in text:
            raise SystemExit(
                f"{path.relative_to(ROOT)} hard-codes current package version {version}; "
                "derive it from .zpkg.toml instead"
            )

    source_tag = (ROOT / ".github" / "workflows" / "zed-source-tag.yml").read_text()
    for forbidden in ("git/ref/tags", "refs/tags/", "Create or verify immutable source tag"):
        if forbidden in source_tag:
            raise SystemExit(
                "zed-source-tag policy may resolve lock metadata but must never create or rewrite release tags"
            )

    release = (ROOT / ".github" / "workflows" / "release-zed.yml").read_text()
    for required in ("Create immutable package tag", "Publish and consume exact Zed release"):
        if required not in release:
            raise SystemExit(f"release-zed workflow lost required owner step: {required}")

    print(f"Zed release policy passed for oresoftware/ores-middleware {version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
