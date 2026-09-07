# Rust middleware / browser-server telemetry interoperability

Tracking: DEN-3432. Source: ores-otel/ores.otel.log#56. External WASM and
Leptos/Dioxus consumers: ores-otel-test/ores-otel-log-rust-test#4.

Both the middleware's `next-loggers` dependency and `ores-otel-web` must resolve
to the same Git source identity. The reviewed identity is
`https://github.com/ores-otel/ores.otel.log` at
`74bc1c421f5a56be28cade1a47f5d45de51f3b4b`. Its Rust SDK requires serde_json
1.0.151. Mixing the older logger revision, or independently pinning another
copy of the context carrier, is not a valid integration.

`src/rust/tests/otel_web_interop.rs` composes the real Axum audit middleware
with `ores_otel_web::server::install_with_logger`. It passes a Logger imported
through `ores_middleware::otel` to the bridge, so Rust type identity itself is
checked. Two interleaved requests must retain their own trace IDs after an
await, both handler and bridge records must be correlated, query values must
stay out of the emitted records, and the caller's task context must be restored.
Synthetic in-process HTTP uses test-only TLS settings; it does not change the
production default, enable test authentication, or weaken forwarded-header
validation, authorization or rate-limit policy.

Run the combined test and the complete Rust suite:

```sh
cargo test -p ores-middleware --test otel_web_interop --all-features --locked
cargo test --workspace --all-targets --all-features --locked
cargo test -p ores-middleware --no-default-features --locked
```

The existing `otel_adversarial` suite imports Axum and therefore declares
`required-features = ["axum"]`; all five of its tests remain in default and
all-feature runs. Portable core and rate-limit tests also run without default
features. No HTTP adapter is silently enabled to make that configuration pass.

The scoped maintenance workflow resolves only the reviewed serializer change,
asks Cargo to generate the lock, executes these tests, verifies one logger
source, and pushes only the generated lock and formatted new test. It cannot
write main or merge a PR. The ordinary repository-wide language, schema,
persistence and package-conformance gates remain separate merge requirements.

This is request/log correlation, not an OTLP span exporter or production
collector delivery certificate. Browser exporter ownership, consent/redaction,
queue/backoff, streaming response-body context and detached tasks remain the
application's explicit responsibility.
