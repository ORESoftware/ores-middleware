#!/usr/bin/env python3
"""Install the Rust ores-middleware adapter at one live Axum boundary.

This helper intentionally performs only source/configuration edits. Callers must run
formatting and compile/tests in the target repository before committing the result.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path

DEPENDENCY_NAME = "ores-middleware"
DEPENDENCY_URL = "https://github.com/ORESoftware/ores-middleware"
DOC_PATH = Path("docs/ores-middleware.md")


class RolloutError(RuntimeError):
    pass


@dataclass(frozen=True)
class Call:
    start: int
    open_paren: int
    close_paren: int
    arguments: tuple[tuple[int, int], ...]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--call", default="axum::serve")
    parser.add_argument("--router-argument", type=int, default=1)
    parser.add_argument("--router-var")
    parser.add_argument(
        "--error-mode",
        choices=("question", "expect"),
        default="question",
        help="Use ? in Result-returning functions or expect in infallible entrypoints.",
    )
    parser.add_argument(
        "--rate-limit-mode",
        choices=("audit", "configured"),
        default="audit",
        help=(
            "Use the non-enforcing Axum audit adapter by default; choose configured "
            "only in a separately reviewed activation change."
        ),
    )
    parser.add_argument("--service-name", default='env!("CARGO_PKG_NAME")')
    return parser.parse_args()


def _scan_matching(source: str, open_index: int) -> int:
    pairs = {"(": ")", "[": "]", "{": "}"}
    stack = [source[open_index]]
    quote: str | None = None
    escaped = False
    line_comment = False
    block_comment_depth = 0
    index = open_index + 1
    while index < len(source):
        char = source[index]
        nxt = source[index + 1] if index + 1 < len(source) else ""
        if line_comment:
            if char == "\n":
                line_comment = False
            index += 1
            continue
        if block_comment_depth:
            if char == "/" and nxt == "*":
                block_comment_depth += 1
                index += 2
                continue
            if char == "*" and nxt == "/":
                block_comment_depth -= 1
                index += 2
                continue
            index += 1
            continue
        if quote:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == quote:
                quote = None
            index += 1
            continue
        if char == "/" and nxt == "/":
            line_comment = True
            index += 2
            continue
        if char == "/" and nxt == "*":
            block_comment_depth = 1
            index += 2
            continue
        if char in ('"', "'"):
            quote = char
            index += 1
            continue
        if char in pairs:
            stack.append(char)
        elif char in pairs.values():
            if not stack or pairs[stack[-1]] != char:
                raise RolloutError(f"unbalanced delimiter at byte {index}")
            stack.pop()
            if not stack:
                return index
        index += 1
    raise RolloutError("unterminated call expression")


def _top_level_arguments(
    source: str, open_index: int, close_index: int
) -> tuple[tuple[int, int], ...]:
    spans: list[tuple[int, int]] = []
    start = open_index + 1
    stack: list[str] = []
    pairs = {"(": ")", "[": "]", "{": "}"}
    quote: str | None = None
    escaped = False
    index = start
    while index < close_index:
        char = source[index]
        if quote:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == quote:
                quote = None
            index += 1
            continue
        if char in ('"', "'"):
            quote = char
        elif char in pairs:
            stack.append(char)
        elif char in pairs.values():
            if stack and pairs[stack[-1]] == char:
                stack.pop()
        elif char == "," and not stack:
            spans.append((start, index))
            start = index + 1
        index += 1
    if source[start:close_index].strip() or spans:
        spans.append((start, close_index))
    return tuple(spans)


def find_call(source: str, call_name: str) -> Call:
    marker = f"{call_name}("
    search_from = 0
    while True:
        start = source.find(marker, search_from)
        if start < 0:
            raise RolloutError(f"live {call_name}(...) boundary not found")
        line_start = source.rfind("\n", 0, start) + 1
        prefix = source[line_start:start].lstrip()
        if not prefix.startswith("//"):
            open_paren = start + len(call_name)
            close_paren = _scan_matching(source, open_paren)
            return Call(
                start=start,
                open_paren=open_paren,
                close_paren=close_paren,
                arguments=_top_level_arguments(source, open_paren, close_paren),
            )
        search_from = start + len(marker)


def _terminator(error_mode: str) -> str:
    return "?" if error_mode == "question" else '.expect("valid ORES middleware configuration")'


def _installer(rate_limit_mode: str) -> str:
    if rate_limit_mode == "audit":
        return "ores_middleware::frameworks::axum_audit::install_from_env"
    return "ores_middleware::frameworks::axum::install_from_env"


def install_expression(
    router: str,
    service_name: str,
    error_mode: str,
    indent: str,
    rate_limit_mode: str,
) -> str:
    inner = indent + "    "
    return (
        f"{_installer(rate_limit_mode)}(\n"
        f"{inner}{router.strip()},\n"
        f"{inner}{service_name},\n"
        f"{indent}){_terminator(error_mode)}"
    )


def _wrap_associated_function(
    argument: str,
    service_name: str,
    error_mode: str,
    indent: str,
    rate_limit_mode: str,
) -> str | None:
    marker = "::into_make_service"
    method = argument.find(marker)
    if method < 0:
        return None
    open_paren = argument.find("(", method)
    if open_paren < 0:
        return None
    try:
        close_paren = _scan_matching(argument, open_paren)
    except RolloutError:
        return None
    if argument[close_paren + 1 :].strip():
        return None
    inner = argument[open_paren + 1 : close_paren].strip()
    if not inner:
        return None
    wrapped = install_expression(
        inner, service_name, error_mode, indent + "    ", rate_limit_mode
    )
    return argument[: open_paren + 1] + "\n" + indent + "    " + wrapped + ",\n" + indent + ")"


def wrap_router_argument(
    argument: str,
    service_name: str,
    error_mode: str,
    indent: str,
    rate_limit_mode: str,
) -> str:
    argument = argument.strip()
    associated = _wrap_associated_function(
        argument, service_name, error_mode, indent, rate_limit_mode
    )
    if associated is not None:
        return associated
    method = re.search(r"\.into_make_service(?:_with_connect_info)?", argument)
    if method:
        router = argument[: method.start()].strip()
        suffix = argument[method.start() :]
        return (
            install_expression(
                router, service_name, error_mode, indent, rate_limit_mode
            )
            + suffix
        )
    return install_expression(
        argument, service_name, error_mode, indent, rate_limit_mode
    )


def insert_router_shadow(
    source: str,
    call: Call,
    router_var: str,
    service_name: str,
    error_mode: str,
    rate_limit_mode: str,
) -> str:
    before = source[: call.start]
    conversion = re.compile(
        rf"(?m)^(?P<indent>[ \t]*)let\s+[A-Za-z_][A-Za-z0-9_]*\s*=\s*{re.escape(router_var)}\s*\.into_make_service"
    )
    matches = list(conversion.finditer(before))
    if matches:
        insertion = matches[-1].start()
        indent = matches[-1].group("indent")
    else:
        insertion = source.rfind("\n", 0, call.start) + 1
        line_prefix = source[insertion:call.start]
        indent = re.match(r"[ \t]*", line_prefix).group(0)
    expression = install_expression(
        router_var, service_name, error_mode, indent, rate_limit_mode
    )
    statement = f"{indent}let {router_var} = {expression};\n"
    return source[:insertion] + statement + source[insertion:]


def patch_source(
    path: Path,
    call_name: str,
    argument_index: int,
    router_var: str | None,
    service_name: str,
    error_mode: str,
    rate_limit_mode: str,
) -> None:
    source = path.read_text()
    if (
        "ores_middleware::frameworks::axum::install_from_env" in source
        or "ores_middleware::frameworks::axum_audit::install_from_env" in source
    ):
        raise RolloutError(f"{path} already contains the ores-middleware installation")
    call = find_call(source, call_name)
    if router_var:
        path.write_text(
            insert_router_shadow(
                source,
                call,
                router_var,
                service_name,
                error_mode,
                rate_limit_mode,
            )
        )
        return
    if argument_index < 0 or argument_index >= len(call.arguments):
        raise RolloutError(
            f"{call_name} has {len(call.arguments)} top-level arguments; cannot wrap index {argument_index}"
        )
    start, end = call.arguments[argument_index]
    line_start = source.rfind("\n", 0, call.start) + 1
    call_indent = re.match(r"[ \t]*", source[line_start:call.start]).group(0)
    argument_indent = call_indent + "    "
    replacement = "\n" + argument_indent + wrap_router_argument(
        source[start:end],
        service_name,
        error_mode,
        argument_indent,
        rate_limit_mode,
    )
    replacement += ",\n" + call_indent
    path.write_text(source[:start] + replacement + source[end:])


def patch_manifest(path: Path, revision: str) -> None:
    manifest = path.read_text()
    dependency = (
        f'{DEPENDENCY_NAME} = {{ git = "{DEPENDENCY_URL}", rev = "{revision}", '
        'features = ["axum"] }'
    )
    pattern = re.compile(rf"(?m)^{re.escape(DEPENDENCY_NAME)}\s*=.*$")
    if pattern.search(manifest):
        manifest = pattern.sub(dependency, manifest, count=1)
    else:
        dependencies = re.search(r"(?m)^\[dependencies\]\s*$", manifest)
        if not dependencies:
            raise RolloutError(f"{path} has no [dependencies] section")
        manifest = (
            manifest[: dependencies.end()]
            + "\n"
            + dependency
            + manifest[dependencies.end() :]
        )
    path.write_text(manifest)


def patch_gitignore(path: Path) -> None:
    text = path.read_text() if path.exists() else ""
    lines = text.splitlines()
    if not any(line.strip().rstrip("/") in {"target", "/target"} for line in lines):
        if text and not text.endswith("\n"):
            text += "\n"
        text += "/target/\n"
        path.write_text(text)


def write_documentation(
    revision: str, source: Path, manifest: Path, rate_limit_mode: str
) -> None:
    DOC_PATH.parent.mkdir(parents=True, exist_ok=True)
    mode_text = (
        "The shared rate-limit decision is forcibly disabled by the audit adapter; "
        "existing service-owned limiters remain authoritative. Activation requires a "
        "separate reviewed change to configured mode."
        if rate_limit_mode == "audit"
        else "The shared rate-limit decision follows the validated runtime configuration."
    )
    DOC_PATH.write_text(
        f"""# Shared request middleware

This server installs `ORESoftware/ores-middleware` at the live Axum boundary in `{source}`, using `{manifest}` and immutable central commit `{revision}`.

The shared layer provides request/trace context, crash recovery, deadlines, streaming payload limits, security headers, compression, ETags, RED telemetry hooks, rate/idempotency ports, and integration ports for shared-auth, opto-sync, and ores-otel. Existing service-specific authentication, authorization, rate limits, and telemetry remain in place beneath the shared request-lifecycle layer.

{mode_text}

Production must set `ORES_MIDDLEWARE_ENV=production` and explicitly choose `ORES_MIDDLEWARE_TLS_MODE=in-process` or `trusted-proxy`. Trusted-proxy mode also requires `ORES_MIDDLEWARE_TRUSTED_PROXY_CIDRS`; forwarded transport headers from other peers are rejected. Development defaults to explicitly disabled TLS enforcement rather than trusting public forwarded headers.

TypeSpec and JSON Schema/OpenAPI are independent, peer, human-authored contract authorities. Any authority, generated-artifact, or runtime-descriptor discrepancy fails closed. Governing instructions: `ORESoftware/my-ai/AGENTS.md`.
"""
    )


def main() -> int:
    args = parse_args()
    source = args.source.resolve()
    manifest = args.manifest.resolve()
    root = Path.cwd().resolve()
    for path in (source, manifest):
        if root not in path.parents and path != root:
            raise RolloutError(f"{path} is outside {root}")
        if not path.is_file():
            raise RolloutError(f"required file does not exist: {path}")
    if not re.fullmatch(r"[0-9a-f]{40}", args.revision):
        raise RolloutError("revision must be a full lowercase 40-hex commit SHA")
    patch_manifest(manifest, args.revision)
    patch_source(
        source,
        args.call,
        args.router_argument,
        args.router_var,
        args.service_name,
        args.error_mode,
        args.rate_limit_mode,
    )
    patch_gitignore(Path(".gitignore"))
    write_documentation(
        args.revision, args.source, args.manifest, args.rate_limit_mode
    )
    print(
        json.dumps(
            {
                "source": str(args.source),
                "manifest": str(args.manifest),
                "documentation": str(DOC_PATH),
                "revision": args.revision,
                "rateLimitMode": args.rate_limit_mode,
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except RolloutError as error:
        print(f"rollout error: {error}", file=sys.stderr)
        raise SystemExit(2)
