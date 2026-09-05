# Polyglot API consistency

JSON Schema defines the wire boundary; SDK manifests define the behavioral API boundary. Both are needed.

A schema can prove that implementations emit compatible data, but it cannot prove that both expose lifecycle methods, preserve context across concurrency, or avoid runtime patching. A prose checklist cannot reliably detect an accidental field rename or an unbounded payload.

## Compatibility layers

1. **Wire compatibility** — `next-loggers/v1` and `next-loggers/batch/v1`.
2. **Logical API compatibility** — logger, event, transport, context, and span operations in `sdk-manifest.schema.json`.
3. **Runtime semantics** — per-language context and concurrency tests.
4. **Telemetry architecture** — explicit adapters, no global providers, no automatic instrumentation, and no monkey patching.
5. **Repository compatibility** — every test-org consumer runs against both `ores-otel/ores.otel.log` and `ORESoftware/next-loggers.ts`.

## Changing an API

A change is complete only when the schema change is backward-compatible or introduces a new versioned discriminator; positive and negative fixtures cover it; every SDK manifest maps the logical operation to native symbols; implemented SDKs have runtime tests; the test-org matrix runs against canonical and legacy repositories; lifecycle and context ownership are documented; and no high-cardinality field becomes a metric or Loki index label.

Do not silently widen an existing schema with unrestricted `additionalProperties`. Extensions belong in bounded `fields`, `context`, or a new versioned schema.

## Context merge semantics

- Maps such as `fields` and `loggedInUser` merge with the inner scope winning.
- `users` append in scope order.
- `traceIds` and `tags` append and deduplicate while retaining first occurrence.
- An inner `traceId` becomes primary without deleting outer trace IDs.
- Scope exit restores the exact previous frame after exceptions, panics, cancellation, and rejected promises.
- Concurrent tasks, threads, and processes must not observe one another's mutable context.

A runtime that cannot provide concurrent isolation must declare `sequential-only` and cannot set `promotion.ready` to true.

## Transport lifecycle

All transports implement the same logical lifecycle: `write(record)` accepts one complete record, `flush()` drains enqueued work, `flushOnExit(records)` receives records recovered during shutdown, and `close()` is idempotent and releases only resources the transport owns.

A logger adapter must not shut down an application-global OTEL provider. Optional telemetry failures may be diagnosed, but they must not replace the primary logging or application result.
