# Middleware configuration hardening — Choice C

This document records the reviewed separation between package metadata, repository-local orchestration, runtime middleware policy, and provider-specific policy.

## Authority split

- `.zpkg.toml`: package coordinates, immutable dependencies, build/install/publish/test mechanics only.
- `.ores-mw.toml`: repository/target orchestration, `flags-2-env` contract binding, and environment-key metadata only.
- referenced middleware stack JSON: cross-cutting runtime middleware policy governed by independently authored TypeSpec + JSON Schema authorities and TJSV.
- `.ores-rl.toml`: rate-limit provider/store policy.
- `.ores-lru.toml`: cache/SWR/eviction/key policy.
- `.auth-shared.toml`: JWT/OAuth/M2M policy.
- `.ores-otel.toml`: telemetry/export/propagation policy.
- `.opto-sync.toml`: synchronization/observation policy.

No secret value belongs in any committed manifest. Secret bindings may name environment/secret-store keys but must not become CLI flags.

## Implemented on this branch

- Added root `.cli-flags.toml` using the canonical `flags-2-env` shape.
- Added `[flags2env]` metadata to `.ores-mw.toml` with `argv-over-env` precedence and audit required.
- Added typed `[[env]]` metadata for the runtime keys already consumed by Rust bootstrap.
- Kept `ORES_MIDDLEWARE_RATE_LIMIT_HMAC_SECRET` secret-store/environment-only and deliberately absent from `.cli-flags.toml`.
- Preserved the existing referenced stack JSON as the runtime-policy authority rather than embedding provider thresholds into `.ores-mw.toml` or `.zpkg.toml`.

## Initial execution findings

1. The current middleware contract has one general `max_body_bytes` limit. Rust Axum adapters install this as the request body limit, while the core pipeline checks declared content length. Parser-specific JSON/XML/Protobuf/MessagePack quotas and decompression-bomb limits remain follow-up work (#153).
2. Current Rust request/header paths found in the lowercase audit use canonical values such as `x-forwarded-for` and `x-request-id`; the fleet-wide/all-adapter audit remains tracked in #163.
3. Rust-first tooling migration is incomplete. Current main still contains Python invocations in `Justfile`, `.zpkg.toml`, `package.json`, CI/TJSV workflows, rollout workflows, and repository scripts. Do not add new Python authority; migrate these under #18.
4. Hosted Actions on PR #154 are presently failing admission before jobs start. At least one inspected failed workflow run reports `total_count: 0` jobs. This is runner/admission evidence, not a code-test failure or pass.

## New hardening workstream

The following tasks were opened and started from the Choice C audit:

- #152 canonical flags-2-env contract + env metadata validation
- #153 parser-specific quotas and decompression-bomb limits
- #155 Shared Auth/M2M reference boundary
- #156 cache SWR/stale-if-error reference boundary
- #157 deterministic precedence and provenance receipts
- #158 immutable hot-reload snapshots
- #159 schema-version migration and downgrade rejection
- #160 secret redaction and non-secret config fingerprints
- #161 provider-policy reference/version/digest validation
- #162 bounded route-class inheritance/overrides
- #163 lowercase header audit across adapters
- #164 ores-cli implementation/config consistency audit
- #165 Rust-first property/fuzz parser hardening
- #166 six-runtime effective-config decoder parity
- #167 fail-open/fail-closed dependency matrix

## Promotion rule

Do not mark a configuration change complete because a workflow badge is green or red. Promotion requires exact-head evidence that:

1. both authored authorities remain independently valid and TJSV reports no unexplained discrepancy;
2. config normalization/admission succeeds on the exact candidate;
3. negative fixtures fail for the expected bounded reason;
4. secrets are not reflected in argv, logs, receipts, generated artifacts, or errors;
5. every runtime touched by the change passes its conformance witness;
6. provider-specific policy remains in its owning config family rather than being copied into `.zpkg.toml` or `.ores-mw.toml`.
