# ORES Middleware Contract and Server Rollout

## Acceptance target

This record verifies that the standard ORES middleware SDK is implemented in every language currently in scope, enforced by **independent, peer TypeSpec and JSON Schema/OpenAPI authorities**, covered by compile-time and runtime conformance gates, and staged for review at the actual router boundary of at least 20 servers across at least 6 GitHub organizations.

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

## Staged server rollout

### Evidence classes

This section records **staged-head evidence** at the immutable pull-request heads shown in the table. `PASS` means that the listed draft PR head contained the middleware installation and its recorded workflow completed successfully. It does **not** assert that the target repository's current default branch contains that change, that the PR was merged, or that production deployment occurred.

[Issue #7](https://github.com/ORESoftware/ores-middleware/issues/7) records a separate **live/default-branch source audit**. A repository counts as live only when its current branch contents contain the middleware adapter call, immutable dependency pin, adoption documentation, and no temporary rollout workflow. Consequently, this document can truthfully report 20/20 staged PR heads while the source audit reports a smaller live-adoption count.

Do not add staged and live counts together, promote a staged PR to live based on workflow status alone, or describe a draft PR as deployed. Future revisions must preserve the immutable PR head, workflow conclusion, audit date, and evidence class for every row.

All staged downstream PRs:

- pin the immutable middleware revision above;
- install `ores_middleware::frameworks::axum::install_from_env` at the actual Axum router boundary;
- fail startup on invalid middleware configuration;
- compile against all seven standard SDK operations;
- validate the canonical descriptor and capabilities at runtime;
- construct the Axum middleware adapter from validated canonical defaults at runtime;
- remain draft until deployment TLS/trusted-proxy policy is reviewed.

| # | Organization | Server | PR | Head | Staged-head workflow audit |
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

**Staged rollout total:** 20 draft PR heads across 11 GitHub organizations. **Staged-head workflow audit:** 20/20 PASS. The live/default-branch count is maintained separately in issue #7.

## Deployment gate

The PRs intentionally remain draft even after code-level CI passes. Before production merge/deployment, each service must explicitly choose one of:

- `ORES_MIDDLEWARE_TLS_MODE=in-process`; or
- `ORES_MIDDLEWARE_TLS_MODE=trusted-proxy`, together with explicit `ORES_MIDDLEWARE_TRUSTED_PROXY_CIDRS`.

A service must not infer trusted proxy behavior merely from forwarded headers. Invalid or incomplete middleware configuration is a startup error.

## Completion checklist

- [x] Standard SDK surface defined by peer TypeSpec and JSON Schema/OpenAPI authorities.
- [x] Authority discrepancy comparison is fail-closed.
- [x] Rust implementation and native tests.
- [x] TypeScript/JavaScript implementation and native tests.
- [x] Go implementation and native tests.
- [x] Gleam implementation and native tests.
- [x] Elixir implementation and native tests.
- [x] Erlang implementation and native tests.
- [x] Compile-time and runtime conformance workflow across every language.
- [x] Middleware change staged and verified at the actual router boundary in 20 draft PR heads.
- [x] Staged rollout spans 11 organizations, exceeding the 6-organization requirement.
- [x] All 20 staged downstream repository workflows passed at their recorded heads.
- [ ] Merge downstream drafts after deployment TLS/trusted-proxy policy review.
- [ ] Confirm each merged server through issue #7's live/default-branch source audit before counting it as adopted.
