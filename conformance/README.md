# Conformance

`contracts/` is the authority for API and data-shape semantics in `ORESoftware/ores-middleware`. `conformance/` is the shared behavioral corpus that runtime implementations, adapters, and compatibility layers must consume. `governance/polyglot-participants.v1.json` is the fail-closed participant registry that binds every first-class language under `src/` to this contract/conformance boundary, its Zed target, and an executable contract-conformance CI job.

## Rules

1. Keep shared behavioral vectors under `conformance/cases/` and feed the same case bytes to every implementation under test.
2. Do not maintain runtime-specific golden vectors. A case may describe implementation-neutral inputs and a normalized expected receipt, but the expected result must be shared.
3. Before promotion, bind runtime evidence to the exact current contract inputs and conformance-case digests. Stale evidence fails closed.
4. Missing evidence from any runtime or adapter declared required by the promotion gate is a failure, not a skip.
5. Generated reports, normalized runtime receipts, parity reports, and other artifacts are evidence only. They do not become contract or conformance authority.
6. `contracts/` remains authoritative for structure and wire shape; `conformance/` owns shared behavioral expectations. Neither directory silently rewrites the other.
7. Every immediate language/package directory under `src/` must be present in `governance/polyglot-participants.v1.json` or be explicitly ignored there with a reason. The governed participant set must exactly match `requiredParticipants` in `conformance/manifest.v1.json`.
8. Every governed participant must expose the same semantic operation categories through its `adapter.manifest.json`, have a matching `.zpkg.toml` target, and be exercised by the repository's `contract-conformance` workflow. Adding a language without all of those bindings fails closed.

The current participant set is Rust, TypeScript, Go, Gleam, Elixir, and Erlang. `scripts/check-polyglot-governance.mjs` derives and validates that set rather than allowing CI, Zed packaging, and conformance metadata to drift independently.

The initial `cases/bootstrap.v1.json` case establishes this repository-level authority boundary only. It does **not** prove domain-level runtime equivalence. Coverage therefore remains honestly marked `scaffold-only` until domain-specific behavior cases and digest-bound runtime evidence are added. Add domain-specific cases as normalized inputs plus normalized expected receipts, then make each supported implementation execute the same corpus.

Where `contracts/instances/` (or another shared contract-instance corpus) already exists, conformance runners should reuse or reference those exact bytes instead of creating divergent runtime-local copies.
