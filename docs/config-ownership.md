# Middleware configuration ownership

This repository uses several configuration surfaces deliberately. They are not interchangeable.

## The boundary

| Concern | Owning configuration |
| --- | --- |
| package coordinates, immutable dependencies, build/install/publish/test mechanics | `.zpkg.toml` |
| repository/target middleware orchestration, `flags-2-env` binding, environment-key metadata | `.ores-mw.toml` |
| cross-cutting middleware runtime stack | referenced middleware stack JSON governed by the independent TypeSpec + JSON Schema authorities and TJSV |
| authentication/JWT/OAuth/M2M policy | `.auth-shared.toml` |
| rate-limit algorithms, capacities, route policies, stores and failure posture | `.ores-rl.toml` |
| Redis/LRU cache, TTL/SWR, eviction and key policy | `.ores-lru.toml` |
| telemetry/export/propagation policy | `.ores-otel.toml` |
| command-line flags and argv-to-env mapping | `.cli-flags.toml` / `flags-2-env` |

`.zpkg.toml` must never become a runtime middleware configuration file. The package manifest may declare dependencies on middleware/auth/rate-limit/cache packages, but it must not contain `[middleware]`, `[middleware.auth]`, `[middleware.rate_limit]`, `[middleware.cache]`, `[middleware.payload]`, or aliases intended to carry the same runtime policy.

The repository now enforces that rule with `scripts/check_config_ownership.py` and the `config-ownership` workflow. The check parses TOML semantically; it does not use substring matching, so comments, repository URLs and a dependency such as `oresoftware/ores-middleware` do not create false positives.

## Where the example settings go

A configuration shaped like this must **not** be moved wholesale from `.zpkg.toml` into a second giant TOML authority. Split it by ownership.

### Authentication

Selection/placement of the authentication stage is middleware orchestration. Detailed authentication policy belongs to Shared Auth. Fields such as token/header policy, JWKS location, M2M enforcement and route exemptions belong in `.auth-shared.toml` or in the independently governed stack representation that binds the Shared Auth integration.

Canonical authored HTTP header names are lowercase, for example `authorization`; inbound matching remains case-insensitive.

### Rate limiting

Rate-limit enablement/placement is middleware orchestration. Algorithm, default capacity/window, client/principal keying, Redis/store selection, local fallback behavior and route-specific limits belong to `.ores-rl.toml` and the `ores-rate-limit` authority. Do not duplicate those rules in `.zpkg.toml` or application code.

Canonical authored headers are lowercase, for example `x-forwarded-for`.

### Cache

Cache enablement/placement is middleware orchestration. TTL, stale-while-revalidate, eviction, Redis/store and cache-key policy belong to `.ores-lru.toml` and the cache package authority. A bypass header, if the consumer deliberately supports one, is authored canonically lowercase such as `x-bypass-cache` and must still pass the normal security/trust boundary.

### Payload parsing and limits

Payload decode/validation behavior is middleware-owned runtime policy. Today the complete server stack remains represented by the referenced peer-authority JSON `MiddlewareStackConfig`; `.ores-mw.toml` selects the target and stack rather than embedding the complete stack schema. JSON/XML/Protobuf byte limits and content-type matching therefore stay in that governed runtime stack until a first-class middleware-owned TOML projection is independently added to both authored authorities.

This preserves the existing rule: `.ores-mw.toml` is orchestration, not a third authority for the full middleware stack.

## Current `.ores-mw.toml` role

The checked-in manifest currently owns:

- `schema_version` and repository mode;
- explicit client/server targets and roots;
- `stack`, `propagation-only`, or `disabled` target middleware mode;
- the repository-relative `stack_config` reference for a server stack;
- canonical propagation headers for client targets;
- `.cli-flags.toml` binding and precedence metadata;
- typed environment-variable names/metadata, including secret **references/keys only**, never secret values.

Detailed provider policy remains with its provider/config family. Secret values remain at the approved environment/secret-store boundary.

## Fail-closed migration rule

When runtime middleware settings are found in `.zpkg.toml`:

1. remove the runtime table from `.zpkg.toml` without altering package dependency declarations;
2. classify each setting by its owning subsystem;
3. place detailed auth/rate-limit/cache/OTel policy in the corresponding package-owned config;
4. keep full cross-cutting stack policy in the independently validated stack JSON where no first-class package config owns it;
5. use `.ores-mw.toml` only for the orchestration/env-metadata surface its peer authorities actually model;
6. preserve wire names and canonical lowercase header spelling;
7. validate the result with the owner config gates plus `python3 scripts/check_config_ownership.py` and the normal `.ores-mw.toml` TJSV/admission gates.

Do not “fix” ownership by inventing unmodeled `[middleware.auth]` or similar tables in `.ores-mw.toml`: the manifest parser intentionally rejects unknown top-level keys. A new first-class composition section must first be added independently to the TypeSpec and JSON Schema authorities, covered by TJSV and adversarial fixtures, and then wired into runtime admission.
