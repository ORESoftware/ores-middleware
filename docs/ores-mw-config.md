# `.ores-mw.toml` middleware orchestration

`ores-middleware` supports a repository-local `.ores-mw.toml` manifest for selecting middleware behavior per code target. The TOML file is **orchestration**, not a third schema authority. Full server stack configuration still lives in JSON instances governed by the repository's independent human-authored TypeSpec and JSON Schema/OpenAPI authorities.

The normalized manifest itself also has two independent authorities:

- `contracts/ores-mw-config/typespec/main.tsp`
- `contracts/ores-mw-config/json-schema/ores-mw-config.schema.json`

CI runs the real `ORESoftware/typespec-json-schema-validator` at an immutable reviewed commit and fails closed unless declaration inventory, direct comparison, differential probes, and positive/negative fixture coverage all pass.

## Manifest shape

```toml
schema_version = 1
repository_mode = "server-only"
default_target = "api"

[[targets]]
name = "api"
role = "server"
roots = ["server"]
middleware = "stack"
stack_config = "config/middleware.server.json"
```

`repository_mode` is one of `server-only`, `client-only`, or `hybrid`. Every target has an explicit `role`, one or more repository-relative `roots`, and one middleware mode:

- `stack`: full middleware stack. Currently server-only and requires `stack_config` pointing to a JSON `MiddlewareStackConfig`.
- `propagation-only`: safe client/outbound context propagation. Requires an explicit lowercase `propagate_headers` list and never loads a server stack.
- `disabled`: no middleware for that target.

This prevents a browser/client directory from accidentally inheriting server-only authentication, TLS, rate-limit, or fault-injection configuration.

## Separate client and server directories

A hybrid repository with disjoint roots can resolve by source path:

```toml
schema_version = 1
repository_mode = "hybrid"

[[targets]]
name = "api"
role = "server"
roots = ["server"]
middleware = "stack"
stack_config = "config/middleware.server.json"

[[targets]]
name = "browser"
role = "client"
roots = ["web"]
middleware = "propagation-only"
propagate_headers = ["traceparent", "x-request-id"]
```

```sh
python3 scripts/ores_mw_config.py resolve --path server/src/main.rs
python3 scripts/ores_mw_config.py resolve --path web/src/app.ts
```

## Client and server in the same source root

When both roles really share the same root, overlap must be explicit:

```toml
schema_version = 1
repository_mode = "hybrid"
allow_overlapping_roots = true

[[targets]]
name = "api"
role = "server"
roots = ["."]
middleware = "stack"
stack_config = "config/middleware.server.json"

[[targets]]
name = "browser"
role = "client"
roots = ["."]
middleware = "propagation-only"
propagate_headers = ["traceparent", "x-request-id"]
```

Path inference is deliberately rejected when multiple targets match. The caller must select the target explicitly:

```sh
python3 scripts/ores_mw_config.py resolve --target api
python3 scripts/ores_mw_config.py resolve --target browser
```

That fail-closed rule is intentional: a shared source tree must never guess whether it is executing in a client or server role.

## Commands

```sh
# Parse + semantic/path/reference checks for this repository.
python3 scripts/ores_mw_config.py check

# Produce the normalized peer-authority JSON shape.
python3 scripts/ores_mw_config.py normalize

# Compile digest-bound evidence and normalized per-server stack JSON.
python3 scripts/ores_mw_config.py compile --out-dir target/ores-mw

# Full repository admission: Python adversarial tests, compilation, and JSON Schema validation.
npm run ores-mw:check

# TJSV gate policy tests and actual pinned compiler-backed convergence.
npm run ores-mw:tjsv:test
npm run ores-mw:tjsv
```

The compile receipt binds the source TOML, normalized manifest, and every referenced full stack config with SHA-256 digests. The JSON admission step validates the normalized manifest against the new authored Draft 2020-12 schema and validates every compiled `stack` target against `contracts/json-schema/middleware-stack.schema.json`.

## Hardening rules

The loader/compiler fails closed on unknown TOML keys, unsupported versions, malformed target names, duplicate targets/roots/headers, mixed repository-mode roles, disabled default targets, client targets attempting to load the server stack, missing propagation headers, absolute/traversal/backslash paths, unsafe root overlap, symlinked referenced paths, non-regular files, oversized inputs, malformed JSON, and unsupported stack contract versions.

Repository paths use portable `/` separators. A target root may be `.`; stack config files may not. Referenced roots/configs must resolve inside the repository and must not traverse symlinks. Config errors return bounded error codes without dumping TOML, JSON payloads, environment variables, or compiler output.

## Authority boundary

Do not embed the complete middleware stack schema in TOML. `stack_config` is a reference to the existing JSON runtime config so the dense middleware contract remains under the established peer-authority TypeSpec + JSON Schema/OpenAPI policy. The TOML manifest controls **where and in which role** middleware applies; it does not replace the middleware contract itself.
