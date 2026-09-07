# Gleam adapter boundary

The Gleam package now includes the routing-neutral implementation of
`ores.docs-serving/v1` in `src/ores_middleware/docs_serving.gleam`.

The implementation consumes the same request semantics as the Rust,
TypeScript/JavaScript, and Go cores and is exercised against the shared
`../../fixtures/docs-serving-conformance.tsv` corpus during `gleam test`.
Unknown routes pass through without added headers; handled routes preserve the
contract's method, representation, digest, HEAD, cache, and security-header
rules.

Framework-specific Wisp, Mist, Cowboy, and OTP adapters remain intentionally
out of scope until the routing-neutral conformance job is green on the exact
reviewed commit. Adapters must delegate to this core rather than reimplementing
content negotiation or discrepancy handling.
