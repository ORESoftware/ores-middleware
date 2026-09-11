#!/usr/bin/env python3
"""Exercise .ores-otel.toml as runtime configuration, not just TOML syntax.

The test deliberately resolves the server view from common + server, follows the
exporter endpoint_env indirection, and verifies that a secret-ish runtime value
comes from the process environment rather than being embedded in the config.
"""

from __future__ import annotations

import copy
import json
import math
import os
from pathlib import Path
import tomllib
from typing import Any, Mapping

ROOT = Path(__file__).resolve().parents[1]
CONFIG_PATH = ROOT / ".ores-otel.toml"
ALLOWED_TOP_LEVEL = {"version", "common", "client", "server"}


def _merge(base: Mapping[str, Any], override: Mapping[str, Any]) -> dict[str, Any]:
    merged = copy.deepcopy(dict(base))
    for key, value in override.items():
        current = merged.get(key)
        if isinstance(current, dict) and isinstance(value, dict):
            merged[key] = _merge(current, value)
        else:
            merged[key] = copy.deepcopy(value)
    return merged


def load_effective_server(
    path: Path = CONFIG_PATH,
    environ: Mapping[str, str] = os.environ,
) -> dict[str, Any]:
    with path.open("rb") as handle:
        config = tomllib.load(handle)

    if config.get("version") != 1:
        raise AssertionError(".ores-otel.toml must declare version = 1")

    unknown = sorted(set(config) - ALLOWED_TOP_LEVEL)
    if unknown:
        raise AssertionError(f"unknown top-level .ores-otel.toml keys: {unknown}")

    common = config.get("common", {})
    server = config.get("server", {})
    if not isinstance(common, dict) or not isinstance(server, dict):
        raise AssertionError("common and server must be TOML tables")

    effective = _merge(common, server)
    if effective.get("enabled", True) is not True:
        raise AssertionError("server telemetry must be enabled in this repository")

    service_name = effective.get("service_name")
    if not isinstance(service_name, str) or not service_name.strip():
        raise AssertionError("effective server service_name must be a non-empty string")

    logging = effective.get("logging", {})
    if not isinstance(logging, dict):
        raise AssertionError("effective logging config must be a table")
    log_level = logging.get("level", "info")
    if not isinstance(log_level, str) or not log_level.strip():
        raise AssertionError("effective logging.level must be a non-empty string")

    tracing = effective.get("tracing", {})
    if not isinstance(tracing, dict):
        raise AssertionError("effective tracing config must be a table")
    sample_ratio = tracing.get("sample_ratio", 1.0)
    if (
        isinstance(sample_ratio, bool)
        or not isinstance(sample_ratio, (int, float))
        or not math.isfinite(float(sample_ratio))
        or not 0.0 <= float(sample_ratio) <= 1.0
    ):
        raise AssertionError("effective tracing.sample_ratio must be finite and within 0..=1")

    propagators = tracing.get("propagators", [])
    if not isinstance(propagators, list) or not all(isinstance(item, str) for item in propagators):
        raise AssertionError("effective tracing.propagators must be a string array")
    if "tracecontext" not in propagators:
        raise AssertionError("effective tracing.propagators must include tracecontext")

    metrics = effective.get("metrics", {})
    if not isinstance(metrics, dict):
        raise AssertionError("effective metrics config must be a table")
    metrics_enabled = metrics.get("enabled", True)
    if not isinstance(metrics_enabled, bool):
        raise AssertionError("effective metrics.enabled must be boolean")

    exporter = effective.get("exporter", {})
    if not isinstance(exporter, dict):
        raise AssertionError("effective exporter config must be a table")
    protocol = exporter.get("protocol")
    endpoint_env = exporter.get("endpoint_env")
    if not isinstance(protocol, str) or not protocol.strip():
        raise AssertionError("effective exporter.protocol must be a non-empty string")
    if not isinstance(endpoint_env, str) or not endpoint_env.strip():
        raise AssertionError("effective exporter.endpoint_env must be a non-empty string")

    endpoint = environ.get(endpoint_env)
    if endpoint is None or not endpoint.strip():
        raise AssertionError(f"runtime environment must define {endpoint_env}")

    return {
        "service_name": service_name,
        "log_level": log_level,
        "sample_ratio": float(sample_ratio),
        "propagators": propagators,
        "metrics_enabled": metrics_enabled,
        "exporter_protocol": protocol,
        "exporter_endpoint_env": endpoint_env,
        "exporter_endpoint": endpoint,
    }


def main() -> None:
    sentinel = "https://collector.invalid/ores-otel-runtime-config-test"
    environ = dict(os.environ)
    environ["OTEL_EXPORTER_OTLP_ENDPOINT"] = sentinel

    resolved = load_effective_server(environ=environ)

    expected_service = os.environ.get("EXPECTED_OTEL_SERVICE_NAME")
    if expected_service and resolved["service_name"] != expected_service:
        raise AssertionError(
            f"expected service_name {expected_service!r}, got {resolved['service_name']!r}"
        )

    if resolved["exporter_endpoint_env"] != "OTEL_EXPORTER_OTLP_ENDPOINT":
        raise AssertionError("runtime must resolve OTLP endpoint through OTEL_EXPORTER_OTLP_ENDPOINT")
    if resolved["exporter_endpoint"] != sentinel:
        raise AssertionError("runtime endpoint indirection did not consume the supplied environment value")
    if sentinel in CONFIG_PATH.read_text(encoding="utf-8"):
        raise AssertionError("runtime endpoint value must not be embedded in .ores-otel.toml")

    safe_receipt = dict(resolved)
    safe_receipt["exporter_endpoint"] = "<resolved-from-environment>"
    print(json.dumps(safe_receipt, sort_keys=True))


if __name__ == "__main__":
    main()
