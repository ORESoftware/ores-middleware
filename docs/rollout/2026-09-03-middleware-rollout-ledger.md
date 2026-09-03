# ORES middleware rollout ledger — 2026-09-03

## Acceptance model

The rollout is governed by the following fail-closed requirements:

1. TypeSpec and JSON Schema Draft 2020-12/OpenAPI are independent, peer, human-authored authorities. Neither is generated from nor subordinate to the other.
2. Contract compilation, normalized authority comparison, canonical fixture validation, and language descriptor validation must all pass.
3. Every declared language implementation must expose the same seven semantic SDK operations:
   - `descriptor`
   - `defaultConfig`
   - `validateConfig`
   - `createMiddleware`
   - `runWithContext`
   - `currentContext`
   - `capabilities`
4. Every declared language must have compiler/type-system checks and executable runtime middleware tests.
5. A server counts only after its integration PR is merged. Issues, plans, local branches, draft PRs, bootstrap-only PRs, and blocked PRs do not count.
6. A merge is admitted only for the exact tested head, with no unresolved conflict, no requested changes, and no required failing or pending check.
7. Dependency-security failures are fixed in the dependency graph; they are not suppressed, ignored, or waived.

## Central implementation

The central repository has merged the following implementation and verification layers:

| Repository | PR | Scope |
|---|---:|---|
| `ORESoftware/ores-middleware` | #1 | Peer contract authorities; Rust, TypeScript/JavaScript, Go, Gleam, Elixir, and Erlang SDK implementations; compile/runtime conformance |
| `ORESoftware/ores-middleware` | #6 | Initial all-language and 20-server rollout evidence |
| `ORESoftware/ores-middleware` | #28 | Polyglot generators plus SQL/ORM convergence gates |
| `ORESoftware/ores-middleware` | #32 | Request-scoped HTTP, TCP, and WebSocket context boundaries and tests |

Generated artifacts remain evidence rather than authorities. Contract or generated-output disagreement stops the release for evaluation.

## Merged live-server integrations

The verified rollout contains **25 merged server integrations across 15 GitHub organizations**.

| # | Organization | Server repository | Merged PR |
|---:|---|---|---:|
| 1 | `3FA-app` | `3fa-admin-api-server.rs` | #5 |
| 2 | `athlet-o` | `athleto-admin-api-server.rs` | #4 |
| 3 | `athlet-o` | `athleto-admin-web-server.rs` | #4 |
| 4 | `canonical-cloud` | `canonical-admin-api-server.rs` | #4 |
| 5 | `canonical-cloud` | `canonical-admin-web-server.rs` | #4 |
| 6 | `claritas-viz` | `claritas-admin-api-server.rs` | #2 |
| 7 | `claritas-viz` | `claritas-api-server.rs` | #4 |
| 8 | `daedalus-fab` | `daedalus-admin-api-server.rs` | #5 |
| 9 | `daedalus-fab` | `daedalus-admin-web-server.rs` | #4 |
| 10 | `declarative-migrations` | `declmig-admin-api-server.rs` | #4 |
| 11 | `declarative-migrations` | `declmig-admin-web-server.rs` | #4 |
| 12 | `hypesiege` | `hypesiege-admin-api-server.rs` | #4 |
| 13 | `hypesiege` | `hypesiege-admin-web-server.rs` | #4 |
| 14 | `opto-sync` | `opto-sync-admin-api-server.rs` | #4 |
| 15 | `opto-sync` | `opto-sync-admin-web-server.rs` | #4 |
| 16 | `praxonne` | `praxonne-admin-api-server.rs` | #4 |
| 17 | `quaestor-ledger` | `quaestor-admin-api-server.rs` | #4 |
| 18 | `quaestor-ledger` | `quaestor-admin-web-server.rs` | #4 |
| 19 | `sonus-auris` | `sonus-auris-admin-api-server.rs` | #6 |
| 20 | `sonus-auris` | `sonus-auris-admin-web-server.rs` | #5 |
| 21 | `voxletra` | `vxl-admin-api-server.rs` | #4 |
| 22 | `voxletra` | `vxl-admin-web-server.rs` | #4 |
| 23 | `zed-pkg` | `zed-api-server.rs` | #52 |
| 24 | `drone-mngr` | `drone-mngr-admin-api-server.rs` | #2 |
| 25 | `fiducia-cloud` | `fiducia-api-server.rs` | #5 |

This exceeds the minimum acceptance threshold of 20 servers across six organizations. Central, library, infrastructure, policy, and end-to-end-test PRs are intentionally excluded from this count.

## Merged adjacent integration work

| Repository | PR | Scope |
|---|---:|---|
| `shared-auth/shared-auth-api-server.rs` | #9 | Shared authentication integration boundary |
| `ores-otel/ores-lib-core` | #13 | Request-scoped logging, tracing, and OpenTelemetry support |
| `ores-redis-lru-cache/ores-lru-redis.rs` | #19 | Cache integration; superseded PR #17 was closed |
| `ores-chat/ores-chat-e2e` | #2 | End-to-end coverage |
| `ores-chat/ores-chat-lib-core` | #4 | Shared chat library integration |

These PRs are relevant dependencies or verification layers but are not counted as live server integrations.

## Open fail-closed gates

### Dependency security

- `benefactor-cc/benefactor-api-server.rs` #7 was functionally green but blocked by `RUSTSEC-2026-0235` through `rkyv 0.7.46`, reached from the pre-stable SeaORM/Rust Decimal graph.
- A single-use exact-head remediation gate tests an upstream-supported move from SeaORM `2.0.0-rc.37` to `2.0.2`, regenerates `Cargo.lock` through Cargo, runs formatting, Clippy, all-target tests, container builds, and an unsuppressed `cargo audit --deny warnings` twice, and proves `rkyv` is absent.
- It may merge #7 only with `--match-head-commit` after all tests pass. Any failure removes the temporary remediation workflows and leaves the PR unmerged.

### Private cross-repository dependencies

`ORESoftware/ores-middleware` #27 is closed without merge and superseded by draft PR #38. Its complete final history remains reachable as merge ancestry in #38. The semantic replacement preserves the deterministic rate-limit adapter but moves it into an optional standalone Rust integration crate and removes the private Cargo Git transport.

PR #38 is intentionally blocked until all of these are true on one exact head:

- `ores-rate-limit/ores-rl-lib-core` publishes reviewed `v0.1.0` from exact commit `f66103b6ea619a033fc1750219226d53f461a459`;
- the middleware commits a real `.zpkg.lock` recording package version, artifact SHA-256, byte size, archive format, source, VCS tag, and VCS commit;
- CI installs the private package with checksum-verified Zed 0.2.3 using `zed install --frozen --install-mode copy`;
- the optional adapter and the ordinary middleware crate both compile and pass tests;
- every other required repository workflow is green and independent review is clean.

The first exact-head replacement run validated the Zed-only source boundary and then stopped with exit code 2 because the lock did not yet exist. Missing lock or release evidence is not converted into success, and no private Git or authenticated URL rewrite is accepted as a fallback.

Other private-dependency blockers remain:

- `usa-acc/usa-acc-admin-api-server.rs` #4 — `usa-acc-lib-core`
- `scintilla-run/scintilla-api-server.rs` #8 — `shared-auth/shared-auth-clients`
- `sonus-auris/sonus-auris-api-server.rs` #27 — `shared-auth/shared-auth-clients`
- `3FA-app/3fa-api-server.rs` #12 — `shared-auth/shared-auth-clients`; this PR is also bootstrap-only and explicitly not mergeable as production work

Private package authorization must use an approved repository secret or GitHub App identity at the Zed registry boundary. A personal access token must never be committed to a workflow, source file, lockfile, issue, PR body, or log. A native package manifest must not add a second private Git transport when root `.zpkg.toml` and the frozen Zed lock are the dependency authority.

### Workflow approval gates

The following PRs require an explicit GitHub workflow approval before their exact heads can be evaluated:

- `benefactor-cc/benefactor-admin-api-server.rs` #4
- `hhaus-org/hhaus-admin-api-server.rs` #2
- `apostille-me/apme-admin-api-server.rs` #2
- `StreemPilot/streempilot-admin-api-server.rs` #2

They remain open until their required runs execute and return green.

### Infrastructure audits

- `ORESoftware/k8s-cluster` #1457
- `ores-chat/ores-chat-infra` #2

Temporary one-shot audits were pushed for these PRs. Each audit reads the target's exact head and refuses to merge unless the PR is open, non-draft, conflict-free, review-clean, has a nonempty completed all-green check rollup, and remains on the same head immediately before merge. Infrastructure PRs are never counted as live-server integrations.

## Operational conclusion

The critical rollout requirement is satisfied by the merged 25-server/15-organization set. Remaining PRs are deliberately excluded from the count and from merge until their exact blockers are resolved. No security advisory, private-dependency failure, workflow approval, review decision, or merge conflict is bypassed.
