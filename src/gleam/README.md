# Gleam adapter boundary

This directory is reserved for the Gleam implementation of
`ores.docs-serving/v1`. It must remain a thin, routing-neutral core and
must pass `fixtures/docs-serving-conformance.tsv` before any framework adapter
is published.

The first contract PR intentionally lands executable reference cores in Rust,
TypeScript/JavaScript, and Go. The Gleam implementation follows in a separate
PR so review can validate OTP/framework semantics without weakening the shared
discrepancy gate.
