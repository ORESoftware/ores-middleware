# OTel runtime config acceptance (#180)

This document records the executable acceptance surface for the Rust/Axum implementation of canonical `.ores-otel.toml` configuration.

- Canonical config/parser/runtime source: `ores-otel/ores.otel.log@080cc8cd09ef6493aa2a2790bcd06793012d8766`.
- Middleware always resolves the config with `RuntimeRole::Server` for server executables.
- `enabled`, `logging.enabled`, `logging.level`, and `logging.console` affect the constructed request logger.
- `logging.auto_send=true` is rejected until the Rust logger has a reviewed matching runtime behavior.
- `tracing.enabled=false` or `sample_ratio=0` disables OTel delivery decisions without suppressing ordinary non-OTel log transports.
- Fractional sampling is deterministic from the trace ID so all events in one trace receive the same decision.
- `exporter.protocol != none` requires `exporter.endpoint_env`, a present endpoint value, an absolute credential-free `http`/`https` URI with no query/fragment, and an explicit application-owned `Transport` reporting `is_open_telemetry() == true`.
- Middleware never installs a global OpenTelemetry provider and never manufactures collector credentials or headers.
- The executable retains `ServerOtelRuntime` and calls `close()` during graceful shutdown.
- Axum exposes the sampled `RequestLogger` to handlers while retaining backward compatibility for `install_with_ores_logger`.
- `Cargo.lock` must resolve both `oresoftware-next-loggers` and `ores-otel-web` from the same exact Git revision.

Focused gate:

```sh
cargo metadata --locked --format-version 1 >/dev/null
cargo test -p ores-middleware --test otel_runtime_config --all-features --locked
cargo test -p ores-middleware --test otel_web_interop --all-features --locked
cargo test -p ores-middleware --no-default-features --locked
cargo clippy -p ores-middleware --all-targets --all-features --locked -- -D warnings
```

The workflow `.github/workflows/otel-web-interop.yml` runs those checks from the exact PR head with read-only repository permissions and confirms there is one canonical logger source at `080cc8cd`.
