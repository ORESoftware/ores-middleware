# `.ores-mw.toml` environment bindings and `flags-2-env`

The repository-local middleware manifest may declare runtime **environment-variable metadata** without becoming a second command-line parser.

`flags-2-env` and `.cli-flags.toml` remain the sole argv contract. `.ores-mw.toml` can declare which environment keys middleware depends on, their scalar kinds, whether they are required, and whether they are secret.

```toml
schema_version = 1
repository_mode = "server-only"
default_target = "api"

[flags2env]
contract = ".cli-flags.toml"
require_audit = true
precedence = "argv-over-env"

[[env]]
name = "redis_url"
key = "REDIS_URL"
kind = "url"
required = true
secret = true
description = "Redis credential URI supplied by the approved secret-delivery boundary."

[[env]]
name = "port"
key = "PORT"
kind = "integer"
required = false
secret = false
default = "8080"

[[targets]]
name = "api"
role = "server"
roots = ["src"]
middleware = "stack"
stack_config = "config/ores-middleware.stack.json"
```

## Security and precedence

The runtime boundary is intentionally split:

1. explicit argv values are parsed and audited by `flags-2-env`;
2. the ambient process environment / approved secret store supplies runtime environment values;
3. only a `secret = false` binding may carry a non-secret TOML `default`.

A secret binding **must not** be exposed as a `.cli-flags.toml` flag. Repository file checks parse the referenced `.cli-flags.toml` and fail closed if a `secret = true` binding's environment key appears as a CLI flag. Secret values are never read, normalized, emitted, logged, or persisted by the middleware manifest compiler.

The flags contract is intentionally fixed to repository-root `.cli-flags.toml`, `require_audit = true`, and `precedence = "argv-over-env"` in schema version 1. Unknown fields, duplicate binding names, duplicate environment keys, invalid binding/key names, unknown scalar kinds, secret defaults, oversized defaults/descriptions, and env declarations without the flags contract all fail closed.

## Cross-language authority

The normalized env/flags shape is part of the existing `.ores-mw.toml` peer-authority contract:

- human-authored TypeSpec: `contracts/ores-mw-config/typespec/main.tsp`;
- independently human-authored JSON Schema Draft 2020-12: `contracts/ores-mw-config/json-schema/ores-mw-config.schema.json`.

`ORESoftware/typespec-json-schema-validator` continues to compare the two authorities on the exact candidate revision. Generated schema witnesses, Contract IR, and receipts are downstream evidence only.

This extension does not move rate-limit policy, Redis-LRU policy, Shared Auth policy, telemetry policy, or credentials into `.ores-mw.toml`. Those remain in their own authoritative configuration layers.
