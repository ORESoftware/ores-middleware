# Portable middleware runtime and provider profiles

ORES separates **middleware semantics**, **runtime artifacts**, and **provider
integration**. The semantic contract is portable. A build artifact may be
runtime- or provider-specific without becoming a new policy authority.

## Runtime profiles

| Profile | Typical sources | Artifact | Intended use |
| --- | --- | --- | --- |
| `portable_javascript` | TypeScript, JavaScript, Gleam targeting JS | ES module | Cloudflare/workerd and ORES JavaScript hosts |
| `portable_wasm` | Rust and other mature Wasm toolchains | Wasm module/component | portable edge/sandbox execution |
| `native` | Rust, Erlang, Gleam targeting BEAM | native library or BEAM module | first-party Rust/Erlang data plane |
| `isolated_process` | Pony, Java/JVM, Go, .NET, Elixir, arbitrary executables | ORES host-ABI process/RPC endpoint | universal language escape hatch |

Wasm is an important portable profile, not the universal execution model.
Languages should retain their natural runtimes when forcing them through Wasm
would weaken semantics, tooling, supervision, or performance.

## Provider overlays

A package may publish a provider overlay such as `cloudflare` alongside generic
artifacts. Selection prefers a compatible provider overlay and otherwise falls
back to a compatible `generic` artifact. Every artifact carries the same
`semantic_contract_sha256`; digest disagreement fails closed.

Provider-specific APIs stay behind their adapter boundary. For Cloudflare this
includes Worker `env` bindings such as KV, Durable Objects, D1, R2, Queues,
Service Bindings, platform rate limiting, and other Worker-specific facilities.
Portable middleware receives only the narrow ORES provider interfaces.

That means a package may deliberately contain both:

```text
policy/core
  -> portable TypeScript/JavaScript artifact
  -> portable Rust/Wasm artifact
  -> native Rust artifact
  -> BEAM artifact
  -> isolated JVM/Pony process artifact
  -> Cloudflare-specific overlay
```

The overlay may optimize or integrate more deeply with its provider, but it must
pass the same normalized fixtures as the generic implementation for every
semantic behavior the package claims to share.

## Recommended language paths

- TypeScript -> JavaScript for broad edge portability.
- Gleam -> JavaScript for edge portability and Gleam -> BEAM for the native
  Erlang runtime.
- Rust -> Wasm for portable edge execution and Rust -> native for first-party
  ORES routing/middleware.
- Erlang/Gleam-BEAM -> native BEAM for supervision/actor semantics.
- Pony/Java/other runtimes -> isolated process/RPC unless a mature portable
  compilation target is explicitly selected by the package author.

These are recommendations, not language restrictions. Conformance is defined by
the contract and fixtures, not by implementation language.

## Edge proxies are transport adapters

Caddy, NGINX and HAProxy are swappable outer edge adapters. They may own TLS,
socket acceptance, trusted forwarding identity, coarse flood controls, health
checks, connection draining, and proxy-specific lifecycle mechanics. They do
not own application route semantics or middleware ordering.

The ORES Rust/Erlang router remains authoritative for the immutable deployment
generation containing routes, middleware graph, worker revisions and contract
metadata. Swapping Caddy for NGINX or HAProxy therefore cannot change the
application-level generation digest.

## Cloudflare is intentionally distinct

Cloudflare is not treated as a fourth interchangeable local proxy. It is a
provider execution target with its own bindings and deployment model. The
`src/ts/src/providers/cloudflare.ts` adapter converts that provider surface into
the portable `edge_minimal` capability boundary; Cloudflare-native middleware
may live beside it as an explicit provider overlay.

Do not reshape the common middleware contract around Cloudflare bindings. Share
portable policy/core logic where practical and keep genuinely provider-specific
code genuinely provider-specific.
