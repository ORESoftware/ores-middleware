# Migration to `ores-otel/ores.otel.log`

## Repository roles

- **Primary:** `ores-otel/ores.otel.log`
- **Compatibility mirror:** `ORESoftware/next-loggers.ts`
- **Consumer/conformance fleet:** `ores-otel-test/*`

The move changes repository ownership and package branding, not the `next-loggers/v1` wire discriminator.

## Safe migration sequence

1. Install the GitHub app on `ores-otel` and `ores-otel-test` with repository administration, contents, pull requests, metadata, and Actions permissions.
2. Create `ores-otel/ores.otel.log` without an initial commit.
3. Mirror the complete Git object database and all refs from `ORESoftware/next-loggers.ts`.
4. Set `ores-otel/ores.otel.log` as the development primary.
5. Keep the old repository readable and preserve history, releases, tags, issues, and package compatibility.
6. Land the JSON Schema/API-manifest gate in both repositories at an identical commit.
7. Create the repositories in `contracts/migration/test-repository-matrix.json`.
8. Seed each test repository with a consumer harness that checks out both sources at explicit SHAs.
9. Require exact-head conformance checks across at least ten repositories and seven languages before changing package metadata or documentation links.
10. Add a one-way, reviewed mirror workflow from canonical to legacy. Never run bidirectional automatic mirroring.
11. Update package repository/homepage metadata only after canonical Actions, release provenance, and test-org checks are green.
12. Publish renamed packages as additive aliases first; deprecate legacy package names only after a documented support window.

## Mirroring history

```sh
git clone --mirror https://github.com/ores-otel/ores.otel.log.git
cd next-loggers.ts.git
git remote add canonical https://github.com/ores-otel/ores.otel.log.git
git push --mirror canonical
```

After seeding, normal development should use a non-mirror clone with the canonical repository as `origin` and the historical repository as `legacy`.

The compatibility mirror must use a dedicated GitHub App or fine-grained token stored as an organization secret. It must never commit credentials or force-push an unexpected non-ancestor update without a reviewed recovery procedure.

## Test organization

The checked-in matrix defines thirteen repositories across Node.js, browser/workerd, Python, Go, Rust, Java, Dart, Gleam, Erlang, Elixir, Ruby, WASM, and a cross-language wire comparator. Every repository must test both source repositories.

A test repository is green only when it verifies JSON Schema record compatibility, logical API manifest compatibility, context isolation/restoration, explicit OTEL integration, no monkey patching, and lifecycle behavior.

The matrix intentionally exceeds the requested minimum so one temporarily blocked toolchain does not erase language diversity.

## Promotion gate

Canonical promotion is blocked while any SDK manifest has `promotion.ready: false`. Promotion requires exact-head checks, not a successful run from a different commit or mutable branch tip.
