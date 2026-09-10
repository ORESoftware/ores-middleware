#!/usr/bin/env python3
"""Fail-closed compiler/resolver for repository-local .ores-mw.toml."""
from __future__ import annotations

import argparse, hashlib, json, os, posixpath, re, stat as statmod, sys, tempfile, tomllib
from pathlib import Path
from typing import Any, Iterable

SCHEMA_VERSION = 1
STACK_CONTRACT_VERSION = "1.0.0"
MAX_MANIFEST_BYTES = 256 * 1024
MAX_STACK_CONFIG_BYTES = 1024 * 1024
MAX_TARGETS, MAX_ROOTS_PER_TARGET, MAX_PROPAGATED_HEADERS, MAX_PATH_BYTES = 64, 64, 32, 512
TARGET_NAME = re.compile(r"^[a-z][a-z0-9-]{0,62}$")
HEADER_NAME = re.compile(r"^[a-z0-9!#$%&'*+.^_`|~-]+$")
REPOSITORY_MODES = {"server-only", "client-only", "hybrid"}
TARGET_ROLES = {"server", "client"}
MIDDLEWARE_MODES = {"stack", "propagation-only", "disabled"}
TOP_KEYS = {"schema_version", "repository_mode", "allow_overlapping_roots", "default_target", "targets"}
TARGET_KEYS = {"name", "role", "roots", "enabled", "middleware", "stack_config", "propagate_headers"}

class ManifestError(ValueError):
    def __init__(self, code: str, field: str = "") -> None:
        super().__init__(code); self.code, self.field = code, field

def need(ok: bool, code: str, field: str = "") -> None:
    if not ok: raise ManifestError(code, field)

def digest(data: bytes) -> str: return hashlib.sha256(data).hexdigest()
def canonical(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False) + "\n").encode()

def read_bounded(path: Path, limit: int) -> bytes:
    try: st = path.lstat()
    except OSError as exc: raise ManifestError("file-unavailable", str(path)) from exc
    need(statmod.S_ISREG(st.st_mode), "regular-file-required", str(path))
    need(not path.is_symlink(), "symlink-not-allowed", str(path))
    need(st.st_nlink == 1, "independent-file-required", str(path))
    need(st.st_size <= limit, "file-too-large", str(path))
    data = path.read_bytes(); need(len(data) <= limit, "file-too-large", str(path)); return data

def portable_path(value: Any, field: str, *, allow_dot: bool) -> str:
    need(isinstance(value, str), "string-required", field)
    need(bool(value) and value == value.strip(), "nonempty-trimmed-string-required", field)
    need("\x00" not in value and "\\" not in value, "portable-path-required", field)
    need(len(value.encode()) <= MAX_PATH_BYTES and not value.startswith("/"), "relative-path-required", field)
    if value == ".": need(allow_dot, "file-path-required", field); return value
    parts = value.split("/")
    need(all(p not in {"", ".", ".."} for p in parts), "safe-relative-path-required", field)
    need(posixpath.normpath(value) == value, "normalized-relative-path-required", field); return value

def target_name(value: Any, field: str) -> str:
    need(isinstance(value, str) and TARGET_NAME.fullmatch(value) is not None, "invalid-target-name", field); return value

def string_array(value: Any, field: str, maximum: int) -> list[str]:
    need(isinstance(value, list) and 0 < len(value) <= maximum, "array-size", field)
    need(all(isinstance(v, str) for v in value), "string-array-required", field); return list(value)

def root_parts(value: str) -> tuple[str, ...]: return () if value == "." else tuple(value.split("/"))
def overlap(a: str, b: str) -> bool:
    left, right = root_parts(a), root_parts(b); n = min(len(left), len(right)); return left[:n] == right[:n]
def contains(root: str, path: str) -> bool:
    r, p = root_parts(root), root_parts(path); return p[:len(r)] == r

def load_manifest(path: Path) -> tuple[bytes, dict[str, Any]]:
    data = read_bounded(path, MAX_MANIFEST_BYTES)
    try: value = tomllib.loads(data.decode())
    except (UnicodeDecodeError, tomllib.TOMLDecodeError) as exc: raise ManifestError("invalid-toml") from exc
    need(isinstance(value, dict), "object-required"); return data, value

def normalize_manifest(raw: dict[str, Any]) -> dict[str, Any]:
    unknown = sorted(set(raw) - TOP_KEYS); need(not unknown, "unknown-top-level-key", unknown[0] if unknown else "")
    version = raw.get("schema_version"); need(type(version) is int and version == 1, "unsupported-schema-version", "schema_version")
    repo_mode = raw.get("repository_mode"); need(repo_mode in REPOSITORY_MODES, "invalid-repository-mode", "repository_mode")
    allow_overlap = raw.get("allow_overlapping_roots", False); need(type(allow_overlap) is bool, "boolean-required", "allow_overlapping_roots")
    default = raw.get("default_target"); default = target_name(default, "default_target") if default is not None else None
    raw_targets = raw.get("targets"); need(isinstance(raw_targets, list) and 0 < len(raw_targets) <= MAX_TARGETS, "target-count", "targets")
    targets, names = [], set()
    for i, item in enumerate(raw_targets):
        p = f"targets[{i}]"; need(isinstance(item, dict), "target-object-required", p)
        unknown = sorted(set(item) - TARGET_KEYS); need(not unknown, "unknown-target-key", f"{p}.{unknown[0]}" if unknown else p)
        name = target_name(item.get("name"), f"{p}.name"); need(name not in names, "duplicate-target-name", f"{p}.name"); names.add(name)
        role = item.get("role"); need(role in TARGET_ROLES, "invalid-target-role", f"{p}.role")
        enabled = item.get("enabled", True); need(type(enabled) is bool, "boolean-required", f"{p}.enabled")
        middleware = item.get("middleware"); need(middleware in MIDDLEWARE_MODES, "invalid-middleware-mode", f"{p}.middleware")
        roots = [portable_path(v, f"{p}.roots[{j}]", allow_dot=True) for j, v in enumerate(string_array(item.get("roots"), f"{p}.roots", MAX_ROOTS_PER_TARGET))]
        need(len(roots) == len(set(roots)), "duplicate-root", p)
        for j, left in enumerate(roots):
            for right in roots[j+1:]: need(not overlap(left, right), "redundant-overlapping-root", p)
        stack = item.get("stack_config")
        if stack is not None:
            stack = portable_path(stack, f"{p}.stack_config", allow_dot=False); need(stack.endswith(".json"), "json-stack-config-required", f"{p}.stack_config")
        headers = item.get("propagate_headers")
        if headers is not None:
            headers = string_array(headers, f"{p}.propagate_headers", MAX_PROPAGATED_HEADERS); seen = set()
            for j, h in enumerate(headers):
                f = f"{p}.propagate_headers[{j}]"; need(h == h.lower(), "lowercase-header-required", f)
                need(HEADER_NAME.fullmatch(h) is not None, "invalid-header-name", f); need(h not in seen, "duplicate-header", f); seen.add(h)
        if middleware == "stack":
            need(stack is not None, "stack-config-required", p); need(headers is None, "propagate-headers-forbidden", p); need(role == "server", "client-stack-unsupported", p)
        elif middleware == "propagation-only":
            need(stack is None, "stack-config-forbidden", p); need(headers is not None, "propagate-headers-required", p)
        else:
            need(stack is None, "stack-config-forbidden", p); need(headers is None, "propagate-headers-forbidden", p)
        target = {"name": name, "role": role, "roots": roots, "enabled": enabled, "middleware": middleware}
        if stack is not None: target["stackConfig"] = stack
        if headers is not None: target["propagateHeaders"] = headers
        targets.append(target)
    enabled = [t for t in targets if t["enabled"]]; need(bool(enabled), "enabled-target-required", "targets")
    roles = {t["role"] for t in enabled}
    if repo_mode == "server-only": need(roles == {"server"}, "repository-mode-role-mismatch", "repository_mode")
    elif repo_mode == "client-only": need(roles == {"client"}, "repository-mode-role-mismatch", "repository_mode")
    else: need(roles == {"server", "client"}, "hybrid-requires-client-and-server", "repository_mode")
    if default is not None:
        match = [t for t in targets if t["name"] == default]; need(bool(match) and match[0]["enabled"], "default-target-must-be-enabled", "default_target")
    for i, left in enumerate(enabled):
        for right in enabled[i+1:]:
            if any(overlap(a, b) for a in left["roots"] for b in right["roots"]): need(allow_overlap, "overlapping-target-roots", f"{left['name']}:{right['name']}")
    out = {"schemaVersion": 1, "repositoryMode": repo_mode, "allowOverlappingRoots": allow_overlap, "targets": targets}
    if default is not None: out["defaultTarget"] = default
    return out

def checked_repo_path(repo_root: Path, relative: str, kind: str) -> Path:
    root = repo_root.resolve(strict=True); current = root; parts = [] if relative == "." else relative.split("/")
    for i, part in enumerate(parts):
        current /= part
        try: st = current.lstat()
        except OSError as exc: raise ManifestError("referenced-path-unavailable", relative) from exc
        need(not current.is_symlink(), "referenced-symlink-not-allowed", relative)
        if i < len(parts)-1 or kind == "directory": need(statmod.S_ISDIR(st.st_mode), "referenced-directory-required", relative)
        else: need(statmod.S_ISREG(st.st_mode) and st.st_nlink == 1, "referenced-regular-file-required", relative)
    if not parts: need(kind == "directory", "referenced-file-required", relative)
    try: current.resolve(strict=True).relative_to(root)
    except (OSError, ValueError) as exc: raise ManifestError("referenced-path-outside-repository", relative) from exc
    return current

def load_stack_config(repo_root: Path, relative: str) -> tuple[bytes, dict[str, Any]]:
    data = read_bounded(checked_repo_path(repo_root, relative, "file"), MAX_STACK_CONFIG_BYTES)
    try: value = json.loads(data)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc: raise ManifestError("invalid-stack-config-json", relative) from exc
    need(isinstance(value, dict), "stack-config-object-required", relative)
    need(value.get("contractVersion") == STACK_CONTRACT_VERSION, "unsupported-stack-contract-version", relative)
    need(all(k in value for k in ("environment", "requiredCapabilities", "settings", "integrations")), "incomplete-stack-config", relative)
    return data, value

def check_referenced_files(manifest: dict[str, Any], repo_root: Path) -> None:
    for target in manifest["targets"]:
        for root in target["roots"]: checked_repo_path(repo_root, root, "directory")
        if target["middleware"] == "stack": load_stack_config(repo_root, target["stackConfig"])

def resolve_target(manifest: dict[str, Any], *, target_name: str | None, source_path: str | None) -> dict[str, Any]:
    need(not (target_name and source_path), "target-or-path-only"); enabled = [t for t in manifest["targets"] if t["enabled"]]
    if target_name:
        target_name = target_name if TARGET_NAME.fullmatch(target_name or "") else ""; matches = [t for t in enabled if t["name"] == target_name]
        need(len(matches) == 1, "target-not-found", "target"); return matches[0]
    if source_path:
        path = portable_path(source_path, "path", allow_dot=True); matches = [t for t in enabled if any(contains(r, path) for r in t["roots"])]
        need(bool(matches), "target-not-found", "path"); need(len(matches) == 1, "ambiguous-target-explicit-name-required", "path"); return matches[0]
    default = manifest.get("defaultTarget"); need(default is not None, "target-required-without-default"); return resolve_target(manifest, target_name=default, source_path=None)

def require_repo_local_manifest(manifest_path: Path, repo_root: Path) -> None:
    try: root, manifest = repo_root.resolve(strict=True), manifest_path.resolve(strict=True)
    except OSError as exc: raise ManifestError("repository-or-manifest-unavailable") from exc
    need(manifest.parent == root, "repository-root-manifest-required", str(manifest_path))

def prepare_output(repo_root: Path, out_dir: Path) -> Path:
    root = repo_root.resolve(strict=True); candidate = out_dir if out_dir.is_absolute() else repo_root / out_dir; candidate.mkdir(parents=True, exist_ok=True)
    resolved = candidate.resolve(strict=True)
    try: relative = resolved.relative_to(root)
    except ValueError as exc: raise ManifestError("output-outside-repository", str(out_dir)) from exc
    need(bool(relative.parts), "output-cannot-be-repository-root", str(out_dir)); current = root
    for part in relative.parts:
        current /= part; need(not current.is_symlink() and current.is_dir(), "output-directory-required", str(out_dir))
    return resolved

def compile_manifest(manifest_path: Path, repo_root: Path, out_dir: Path) -> dict[str, Any]:
    require_repo_local_manifest(manifest_path, repo_root); source, raw = load_manifest(manifest_path); manifest = normalize_manifest(raw); check_referenced_files(manifest, repo_root)
    normalized = canonical(manifest); rendered = {"manifest.json": normalized}; rows = []
    for target in manifest["targets"]:
        row = {k: target[k] for k in ("name", "role", "enabled", "middleware", "roots")}
        if target["middleware"] == "stack":
            config_source, config = load_stack_config(repo_root, target["stackConfig"]); config_bytes = canonical(config); compiled = f"targets/{target['name']}.json"; rendered[compiled] = config_bytes
            row.update({"stackConfig": target["stackConfig"], "stackConfigSourceSha256": digest(config_source), "stackConfigNormalizedSha256": digest(config_bytes), "compiledPath": compiled})
        elif target["middleware"] == "propagation-only": row["propagateHeaders"] = target["propagateHeaders"]
        rows.append(row)
    receipt = {"schema": "ores.middleware.config-receipt/v1", "status": "passed", "schemaVersion": 1, "manifestSourceSha256": digest(source), "normalizedManifestSha256": digest(normalized), "targetCount": len(rows), "targets": rows}
    rendered["receipt.json"] = canonical(receipt); out_dir = prepare_output(repo_root, out_dir)
    for relative, data in rendered.items():
        dst = out_dir / relative; dst.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(dir=dst.parent, delete=False) as handle: handle.write(data); tmp = Path(handle.name)
        os.chmod(tmp, 0o600); os.replace(tmp, dst)
    return receipt

def resolved_payload(target: dict[str, Any], repo_root: Path) -> dict[str, Any]:
    payload = {"schema": "ores.middleware.resolved-target/v1", "target": target}
    if target["middleware"] == "stack":
        source, config = load_stack_config(repo_root, target["stackConfig"]); normalized = canonical(config)
        payload.update({"stackConfig": config, "stackConfigSourceSha256": digest(source), "stackConfigNormalizedSha256": digest(normalized)})
    return payload

def write_json(value: Any, output: str = "-") -> None:
    data = canonical(value)
    if output == "-": sys.stdout.buffer.write(data)
    else: path = Path(output); path.parent.mkdir(parents=True, exist_ok=True); path.write_bytes(data)

def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="ores-mw-config"); p.add_argument("--manifest", default=".ores-mw.toml"); p.add_argument("--repo-root", default="."); sub = p.add_subparsers(dest="command", required=True)
    check = sub.add_parser("check"); check.add_argument("--no-file-check", action="store_true")
    normalize = sub.add_parser("normalize"); normalize.add_argument("--output", default="-")
    resolve = sub.add_parser("resolve"); group = resolve.add_mutually_exclusive_group(); group.add_argument("--target"); group.add_argument("--path"); resolve.add_argument("--output", default="-")
    compile_cmd = sub.add_parser("compile"); compile_cmd.add_argument("--out-dir", default="target/ores-mw"); return p

def main(argv: Iterable[str] | None = None) -> int:
    args = parser().parse_args(list(argv) if argv is not None else None); manifest_path, repo_root = Path(args.manifest), Path(args.repo_root)
    try:
        _, raw = load_manifest(manifest_path); manifest = normalize_manifest(raw)
        if args.command == "check":
            if not args.no_file_check: require_repo_local_manifest(manifest_path, repo_root); check_referenced_files(manifest, repo_root)
            write_json({"schema": "ores.middleware.config-check/v1", "status": "passed", "schemaVersion": 1, "targetCount": len(manifest["targets"])})
        elif args.command == "normalize": write_json(manifest, args.output)
        elif args.command == "resolve": require_repo_local_manifest(manifest_path, repo_root); write_json(resolved_payload(resolve_target(manifest, target_name=args.target, source_path=args.path), repo_root), args.output)
        else: write_json(compile_manifest(manifest_path, repo_root, Path(args.out_dir)))
        return 0
    except ManifestError as exc:
        write_json({"status": "failed", "code": exc.code, "field": exc.field}); return 2

if __name__ == "__main__": raise SystemExit(main())
