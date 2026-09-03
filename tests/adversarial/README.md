# Adversarial middleware verification

The ordinary language test suites prove adapter behavior once. The adversarial runner repeats those same native suites under strict runtime settings, randomized native test order where supported, high test concurrency, and Go's race detector.

The runner is implemented in Rust so orchestration, option parsing, deterministic seed derivation, process execution, and receipt generation are compiled and type checked.

## Invariants exercised

Each run must preserve the properties asserted by the native adapter tests:

1. Concurrent requests do not share request, trace, user, tenant, or baggage context.
2. Request context is restored after normal completion, rejection, timeout, panic, throw, or transport failure.
3. A request-bound logger and a separately imported file logger observe the same authenticated correlation identifiers while the scope is active.
4. Logger delivery failures do not replace the handler response or exception.
5. Only authenticated `otel.*` baggage propagates; authorization headers, cookies, credentials, raw tokens, request bodies, and arbitrary baggage remain excluded.
6. Runtime-native propagation remains primary: AsyncLocalStorage, `context.Context`, Tokio task-local state and request extensions, and BEAM process context.

## Local use

Run all six native suites three times:

```bash
cargo run --manifest-path tools/middleware-adversarial-runner/Cargo.toml -- \
  --iterations 3 \
  --receipt target/adversarial/receipt.json
```

Run one suite:

```bash
cargo run --manifest-path tools/middleware-adversarial-runner/Cargo.toml -- \
  --language golang \
  --iterations 10
```

Set `ORES_MIDDLEWARE_STRESS_SEED` to replay a recorded run. Each native invocation writes a separate combined stdout/stderr log, and the JSON receipt records the exact Git commit, seed, command, duration, exit status, and aggregate result.

## CI policy

Pull requests and default-branch pushes run three iterations per language. Scheduled runs use ten iterations. Manual runs accept an explicit value from 1 through 100. Any failed iteration fails the job, while all completed logs and the receipt are uploaded even after a test failure.
