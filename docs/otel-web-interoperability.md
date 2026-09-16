# Rust middleware / browser-server telemetry interoperability

Tracking: DEN-3432 and #180. Source: ores-otel/ores.otel.log. External WASM and
Leptos/Dioxus consumers: ores-otel-test/ores-otel-log-rust-test#4.

Both the middleware's `next-loggers` dependency and `ores-otel-web` must resolve
to the same Git source identity. The reviewed identity is
`https://github.com/ores-otel/ores.otel.log` at
`080cc8cd09ef6493aa2a2790bcd06793012d8766`. That revision contains the strict
Rust `.ores-otel.toml` loader plus the shared lookup/parity fixes used by the
TypeScript and Dart config implementations. Mixing an older logger revision,
or independently pinning another copy of the context carrier, is not a valid
integration.

`src/rust/tests/otel_web_interop.rs` composes the real Axum audit middleware
with `ores_otel_web::server::install_with_logger`. It passes a Logger imported
through `ores_middleware::otel` to the bridge, so Rust type identity itself is
checked. Two interleaved requests must retain their own trace IDs after an
await, both handler and bridge records must be correlated, query values must
stay out of the emitted records, and the caller's task context must be restored.
Synthetic in-process HTTP uses test-only TLS settings; it does not change the
production default, enable test authentication, or weaken forwarded-header
validation, authorization or rate-limit policy.

For authored server telemetry configuration, middleware now uses the canonical
Rust loader with `RuntimeRole::Server`. `logging.enabled`, `logging.level` and
`logging.console` are runtime-active. `tracing.sample_ratio` is applied as a
deterministic per-trace decision, so every request event with one trace ID makes
the same OTel-delivery decision without mutable sampler state.

Exporter configuration is deliberately split from provider ownership. A
non-`none` OTLP protocol requires a credential-free absolute `http`/`https`
endpoint from the configured environment variable and an explicit
application-owned transport whose `is_open_telemetry()` is true. Middleware
does not install a global OpenTelemetry provider, invent authorization headers,
or turn endpoint configuration into a network client. This keeps secret and
provider lifecycle ownership in the executable while still making authored
config fail closed.

`logging.auto_send=true` is also fail-closed until the Rust logger exposes a
matching reviewed runtime behavior; silently ignoring that authored setting is
not permitted. Applications using `ServerOtelRuntime` retain the runtime handle
and call `close()` during graceful shutdown so buffered logger transports are
flushed and closed deliberately.

Run the focused and complete Rust suites:

```sh
cargo test -p ores-middleware --test otel_runtime_config --all-features --locked
cargo test -p ores-middleware --test otel_web_interop --all-features --locked
cargo test --workspace --all-targets --all-features --locked
cargo test -p ores-middleware --no-default-features --locked
```

The existing `otel_adversarial` suite imports Axum and therefore declares
`required-features = ["axum"]`; its tests remain in default and all-feature
runs. Portable core and rate-limit tests also run without default features. No
HTTP adapter is silently enabled to make that configuration pass.

This remains request/log correlation and configuration enforcement, not a
claim that middleware itself owns an OTLP collector/provider. Browser exporter
ownership, consent/redaction, queue/backoff, streaming response-body context
and detached tasks remain explicit application responsibilities.
