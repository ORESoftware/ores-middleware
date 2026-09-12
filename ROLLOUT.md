# ORES Middleware Contract and Live Server Rollout

## Current result

The ordered admission program is complete for the shared core and for the first
server-adoption wave:

- independent, reproducible TypeSpec and JSON Schema/OpenAPI generator lanes;
- normalized SQL, SDK/type, Diesel and SeaORM artifacts with fail-closed parity;
- real Diesel/SeaORM compilation and PostgreSQL catalog reconciliation;
- canonical `zed-pkg` parser, pack, install and reproducibility acceptance;
- compile-time and runtime conformance in Rust, TypeScript, Go, Gleam, Elixir
  and Erlang; and
- **20 verified current default branches across 11 GitHub organizations**.

The machine-readable source of truth is
[`rollout/live-adoptions.json`](rollout/live-adoptions.json). Central CI runs
`scripts/check_live_rollout_evidence.py` and its negative tests.

This record does **not** assert that any service is deployed to production.
Source adoption, default-branch verification and production deployment are
separate evidence classes.

## Independent contract authorities

TypeSpec and JSON Schema/OpenAPI are top-level, human-authored peers. Neither is
generated from nor subordinate to the other.

```text
TypeSpec
  -> SQL_T where applicable
  -> Protobuf
  -> gRPC
  -> wire clients

JSON Schema/OpenAPI
  -> interfaces/types/runtime validators
  -> SQL_J where applicable
  -> write clients
```

Generated files are witnesses, not replacement authorities. Every unexplained
source, normalized-type, SQL, Diesel, SeaORM, runtime or installed-package
difference enters `STOPPED_FOR_EVALUATION`; no lane is selected as a fallback.

## Core admission gates

| Gate | Evidence | Status |
|---|---|---|
| Independent generators | each authority is parsed and projected separately | PASS |
| Normalized semantic parity | fields, types, requiredness, enums, keys and constraints | PASS |
| Generated SQL/type parity | SQL_T vs SQL_J and language artifacts | PASS |
| ORM convergence | Diesel and SeaORM within and across authority lanes | PASS |
| Live database convergence | separate PostgreSQL applications and catalog read-back | PASS |
| Canonical Zed acceptance | canonical parser, pack, copy-mode install and repeated digest | PASS |
| Six-language conformance | native compile/runtime suites and shared descriptors | PASS |

The standard SDK surface remains:

- `descriptor`
- `defaultConfig`
- `validateConfig`
- `createMiddleware`
- `runWithContext`
- `currentContext`
- `capabilities`

## Evidence classes

A **staged head** proves only that a proposed PR head compiled. A **verified
current default branch** proves that the merged default branch contains:

1. the live adapter integration;
2. a full immutable `ORESoftware/ores-middleware` revision;
3. `docs/ores-middleware.md`;
4. a successful repository workflow at the evidence PR head; and
5. no temporary rollout control in the evidence merge.

A **production deployment** additionally requires reviewed transport,
authentication, authorization, observability, rate-limit, idempotency and
environment configuration. Deployment is not inferred from Git state.

## Verified current default branches

| # | Repository | Live source | Runtime PR / merge | Evidence PR / merge | Pin | Check |
|---:|---|---|---|---|---|---|
| 1 | `3FA-app/3fa-admin-api-server.rs` | `src/server.rs` | #5 / `340c3ef3b3a2` | #6 / `fe7b0fd84a93` | `84afb50b81ae` | PASS |
| 2 | `canonical-cloud/canonical-admin-api-server.rs` | `src/main.rs` | #4 / `bd9ac797d0c5` | #5 / `58370377997e` | `84afb50b81ae` | PASS |
| 3 | `athlet-o/athleto-admin-api-server.rs` | `src/main.rs` | #4 / `9305f3249dec` | #5 / `ee75c0da618f` | `84afb50b81ae` | PASS |
| 4 | `declarative-migrations/declmig-admin-api-server.rs` | `src/main.rs` | #4 / `7029db29a140` | #5 / `7d492e62220d` | `84afb50b81ae` | PASS |
| 5 | `opto-sync/opto-sync-admin-api-server.rs` | `src/main.rs` | #4 / `dd947b910e29` | #5 / `407bab91b6e6` | `84afb50b81ae` | PASS |
| 6 | `voxletra/vxl-admin-api-server.rs` | `src/main.rs` | #4 / `31d68732f81f` | #5 / `e6be0d8789a1` | `84afb50b81ae` | PASS |
| 7 | `hypesiege/hypesiege-admin-api-server.rs` | `src/main.rs` | #4 / `d10fad17c0d3` | #5 / `581ae1435e1b` | `84afb50b81ae` | PASS |
| 8 | `quaestor-ledger/quaestor-admin-api-server.rs` | `src/server.rs` | #4 / `708e71c414c4` | #5 / `408f97ca6057` | `84afb50b81ae` | PASS |
| 9 | `daedalus-fab/daedalus-admin-api-server.rs` | `src/server.rs` | #5 / `0b5dba95352d` | #6 / `fa961925e4ab` | `84afb50b81ae` | PASS |
| 10 | `sonus-auris/sonus-auris-admin-api-server.rs` | `src/http/ores_middleware.rs` | #6 / `cce337479d09` | #7 / `242a38bf5893` | `a9491ebe150f` | PASS |
| 11 | `praxonne/praxonne-admin-api-server.rs` | `src/server.rs` | #4 / `5dc23f0d9560` | #5 / `e34259ae74b2` | `84afb50b81ae` | PASS |
| 12 | `athlet-o/athleto-admin-web-server.rs` | `src/main.rs` | #4 / `8ff6425199e7` | #5 / `eac633467596` | `84afb50b81ae` | PASS |
| 13 | `canonical-cloud/canonical-admin-web-server.rs` | `src/main.rs` | #4 / `552de8934cde` | #5 / `94ae0a04d3b5` | `84afb50b81ae` | PASS |
| 14 | `declarative-migrations/declmig-admin-web-server.rs` | `src/main.rs` | #4 / `5cc74a797302` | #5 / `f1b119f600c1` | `84afb50b81ae` | PASS |
| 15 | `opto-sync/opto-sync-admin-web-server.rs` | `src/main.rs` | #4 / `7055c3a755cd` | #5 / `c07406d640da` | `84afb50b81ae` | PASS |
| 16 | `voxletra/vxl-admin-web-server.rs` | `src/main.rs` | #4 / `0c26b6780ba7` | #5 / `1a4d3d44622f` | `84afb50b81ae` | PASS |
| 17 | `hypesiege/hypesiege-admin-web-server.rs` | `src/main.rs` | #4 / `d2f197e654d8` | #5 / `91e2da44826b` | `84afb50b81ae` | PASS |
| 18 | `quaestor-ledger/quaestor-admin-web-server.rs` | `src/server.rs` | #4 / `114f9383cd90` | #5 / `022a9bf404b4` | `84afb50b81ae` | PASS |
| 19 | `daedalus-fab/daedalus-admin-web-server.rs` | `src/server.rs` | #4 / `43312ce1ad9a` | #5 / `2482a7905dc7` | `84afb50b81ae` | PASS |
| 20 | `sonus-auris/sonus-auris-admin-web-server.rs` | `src/server.rs` | #5 / `94a23fa314cf` | #6 / `fab0e747bfa7` | `84afb50b81ae` | PASS |

**Verified source-adoption total:** 20 repositories across 11 organizations.

Nineteen rows pin `84afb50b81ae4fba4da2c4cf4f8c7b934a11ddb3`.
The Sonus Auris admin API replacement pins merged-core revision
`a9491ebe150fa0de9b15b5d6be9d00d8b13c464b`; its earlier superseded PR is not
counted separately.

Full 40-character runtime merge commits, evidence heads, evidence merge commits,
workflow names and pins are retained in `rollout/live-adoptions.json`. The
checker rejects shortened revisions, duplicate repositories or evidence
commits, count drift, summary drift and any declared temporary rollout control.

## Production transport gate

Each service must explicitly choose one reviewed production transport model:

```text
ORES_MIDDLEWARE_ENV=production
ORES_MIDDLEWARE_TLS_MODE=in-process
```

or:

```text
ORES_MIDDLEWARE_ENV=production
ORES_MIDDLEWARE_TLS_MODE=trusted-proxy
ORES_MIDDLEWARE_TRUSTED_PROXY_CIDRS=<reviewed CIDR list>
```

Forwarded transport headers do not establish trust on their own. Test-auth
bypass and fault injection remain prohibited in production. Existing
service-specific authentication, authorization, telemetry and domain rate
limits remain in place beneath the shared request-lifecycle boundary.

## Completion checklist

- [x] Independent TypeSpec generator lane.
- [x] Independent JSON Schema/OpenAPI generator lane.
- [x] Normalized SQL and language-type comparison.
- [x] Diesel and SeaORM cross-checks within and across lanes.
- [x] Real PostgreSQL catalog reconciliation.
- [x] Canonical `zed-pkg` parser/pack/install/reproducibility proof.
- [x] Compile-time and runtime conformance in all six languages.
- [x] Twenty live adapter integrations merged.
- [x] Twenty default-branch adoption documents merged.
- [x] Twenty exact evidence heads completed repository checks successfully.
- [x] Rollout spans eleven organizations.
- [x] Static evidence receipt and fail-closed central validator.
- [ ] Review and record production deployment state service by service.

## Acceptance target

This record verifies that the standard ORES middleware SDK is implemented in every language currently in scope, enforced by **independent, peer TypeSpec and JSON Schema/OpenAPI authorities**, covered by compile-time and runtime conformance gates, and staged at the live router boundary of at least 20 servers across at least 6 GitHub organizations.

## Contract authorities

TypeSpec and JSON Schema/OpenAPI are top-level, human-authored contract authorities. Neither is generated from nor subordinate to the other.

The conformance gate:

1. compiles the TypeSpec authority;
2. validates the JSON Schema authority and canonical fixtures;
3. compares the required operation and capability vocabularies from both authorities;
4. stops on any discrepancy;
5. validates each language descriptor against the shared contract;
6. compiles and runs each language implementation's native tests.

## Standard SDK surface

Every language exports these seven operations, using language-idiomatic symbol spelling while preserving the canonical operation identifiers:

- `descriptor`
- `defaultConfig`
- `validateConfig`
- `createMiddleware`
- `runWithContext`
- `currentContext`
- `capabilities`

## Language implementations

| Language | Compile-time gate | Runtime gate | Status |
|---|---|---|---|
| Rust | `cargo check` / compiler-visible public API | native tests, descriptor and middleware behavior | PASS |
| TypeScript / JavaScript | TypeScript compilation and exported interface checks | Node runtime tests and descriptor validation | PASS |
| Go | package compilation and interface assertions | `go test` and descriptor validation | PASS |
| Gleam | Gleam compilation | native tests and descriptor validation | PASS |
| Elixir | Mix compilation | ExUnit/runtime descriptor validation | PASS |
| Erlang | Erlang compilation | EUnit/runtime descriptor validation | PASS |

The immutable middleware revision used by the server rollout is:

```text
84afb50b81ae4fba4da2c4cf4f8c7b934a11ddb3
```

## Server rollout

All downstream PRs:

- pin the immutable middleware revision above;
- install `ores_middleware::frameworks::axum::install_from_env` at the actual Axum router boundary;
- fail startup on invalid middleware configuration;
- compile against all seven standard SDK operations;
- validate the canonical descriptor and capabilities at runtime;
- construct the Axum middleware adapter from validated canonical defaults at runtime;
- remain draft until deployment TLS/trusted-proxy policy is reviewed.

| # | Organization | Server | PR | Head | Workflow audit |
|---:|---|---|---|---|---|
| 1 | 3FA-app | `3fa-admin-api-server.rs` | [#5](https://github.com/3FA-app/3fa-admin-api-server.rs/pull/5) | `961980891b60` | PASS |
| 2 | canonical-cloud | `canonical-admin-api-server.rs` | [#4](https://github.com/canonical-cloud/canonical-admin-api-server.rs/pull/4) | `82ad525bf5ee` | PASS |
| 3 | athlet-o | `athleto-admin-api-server.rs` | [#4](https://github.com/athlet-o/athleto-admin-api-server.rs/pull/4) | `2f116c9c09ca` | PASS |
| 4 | declarative-migrations | `declmig-admin-api-server.rs` | [#4](https://github.com/declarative-migrations/declmig-admin-api-server.rs/pull/4) | `b720fdc272cd` | PASS |
| 5 | opto-sync | `opto-sync-admin-api-server.rs` | [#4](https://github.com/opto-sync/opto-sync-admin-api-server.rs/pull/4) | `a29fdb082f6a` | PASS |
| 6 | voxletra | `vxl-admin-api-server.rs` | [#4](https://github.com/voxletra/vxl-admin-api-server.rs/pull/4) | `5d80bac49011` | PASS |
| 7 | hypesiege | `hypesiege-admin-api-server.rs` | [#4](https://github.com/hypesiege/hypesiege-admin-api-server.rs/pull/4) | `6ec8b81452b7` | PASS |
| 8 | quaestor-ledger | `quaestor-admin-api-server.rs` | [#4](https://github.com/quaestor-ledger/quaestor-admin-api-server.rs/pull/4) | `6bb77e32c24c` | PASS |
| 9 | daedalus-fab | `daedalus-admin-api-server.rs` | [#5](https://github.com/daedalus-fab/daedalus-admin-api-server.rs/pull/5) | `2efdf0ac7344` | PASS |
| 10 | sonus-auris | `sonus-auris-admin-api-server.rs` | [#5](https://github.com/sonus-auris/sonus-auris-admin-api-server.rs/pull/5) | `ee3413d0cdec` | PASS (2 workflows) |
| 11 | praxonne | `praxonne-admin-api-server.rs` | [#4](https://github.com/praxonne/praxonne-admin-api-server.rs/pull/4) | `2c31ee67b6ba` | PASS |
| 12 | athlet-o | `athleto-admin-web-server.rs` | [#4](https://github.com/athlet-o/athleto-admin-web-server.rs/pull/4) | `d5f941d910fe` | PASS |
| 13 | canonical-cloud | `canonical-admin-web-server.rs` | [#4](https://github.com/canonical-cloud/canonical-admin-web-server.rs/pull/4) | `d744429f77f5` | PASS |
| 14 | declarative-migrations | `declmig-admin-web-server.rs` | [#4](https://github.com/declarative-migrations/declmig-admin-web-server.rs/pull/4) | `92a1444f0c41` | PASS |
| 15 | opto-sync | `opto-sync-admin-web-server.rs` | [#4](https://github.com/opto-sync/opto-sync-admin-web-server.rs/pull/4) | `7139977beb02` | PASS |
| 16 | voxletra | `vxl-admin-web-server.rs` | [#4](https://github.com/voxletra/vxl-admin-web-server.rs/pull/4) | `a8d5f595a743` | PASS |
| 17 | hypesiege | `hypesiege-admin-web-server.rs` | [#4](https://github.com/hypesiege/hypesiege-admin-web-server.rs/pull/4) | `8bcf7891de62` | PASS |
| 18 | quaestor-ledger | `quaestor-admin-web-server.rs` | [#4](https://github.com/quaestor-ledger/quaestor-admin-web-server.rs/pull/4) | `b09378bfba76` | PASS |
| 19 | daedalus-fab | `daedalus-admin-web-server.rs` | [#4](https://github.com/daedalus-fab/daedalus-admin-web-server.rs/pull/4) | `e165e5209d54` | PASS |
| 20 | sonus-auris | `sonus-auris-admin-web-server.rs` | [#5](https://github.com/sonus-auris/sonus-auris-admin-web-server.rs/pull/5) | `3fc4c9fb5ce5` | PASS (2 workflows) |

**Rollout total:** 20 servers across 11 GitHub organizations. **Workflow audit:** 20/20 PASS.

## Deployment gate

The PRs intentionally remain draft even after code-level CI passes. Before production merge/deployment, each service must explicitly choose one of:

- `ORES_MIDDLEWARE_TLS_MODE=in-process`; or
- `ORES_MIDDLEWARE_TLS_MODE=trusted-proxy`, together with explicit `ORES_MIDDLEWARE_TRUSTED_PROXY_CIDRS`.

A service must not infer trusted proxy behavior merely from forwarded headers. Invalid or incomplete middleware configuration is a startup error.

