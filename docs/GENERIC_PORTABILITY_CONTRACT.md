# Generic portability contract

The generic middleware surface is a cross-language contract for consumers that do **not** want ORES to choose their HTTP framework, auth SDK, middleware order, request type, response type, or failure model.

Rust, TypeScript, Go, Gleam, Elixir, and Erlang use different language mechanisms, but the semantic contract is the same:

1. **Consumer-owned providers.** The consumer owns the concrete SDK, exact dependency version/revision, credentials, lifecycle, and retry behavior. `ores-middleware` receives only a narrow provider/verification function or interface.
2. **Generic request/response types.** Generic middleware is parameterized over, or remains agnostic to, request/response/context types instead of requiring an ORES request model or one HTTP framework.
3. **Consumer-owned order.** Composition preserves declaration order exactly. The library does not infer that `auth`, `rate-limit`, `request-id`, or another named stage must run first.
4. **Opaque names.** Named middleware metadata is preserved but is not interpreted, deduplicated, sorted, or normalized by the generic composition layer.
5. **Consumer-owned failure semantics.** Rust uses typed `Result`/provider errors; TypeScript may use exceptions or typed result unions; Go uses `error`; Gleam commonly embeds `Result`; Elixir and Erlang preserve consumer tuples/exceptions/terms. The generic layer does not collapse them into a provider-specific error model.
6. **Context stays owned by the caller.** Go propagates `context.Context`; TypeScript contextual middleware receives the consumer context shape; Gleam, Elixir, and Erlang accept caller-owned values; Rust retains typed request/provider state.
7. **No provider SDK dependencies in the generic core.** Generic modules must not import or depend on Supabase, Clerk, Auth0, Firebase, Okta, Cognito, Keycloak, WorkOS, or another concrete identity provider.
8. **No framework ownership in the generic core.** Express/Koa/Fastify, Plug/Phoenix, Cowboy/Ranch, and similar bindings belong in adapters, not generic composition.
9. **Identity chains are valid.** Empty middleware lists preserve the handler unchanged or semantically unchanged.
10. **Duplicate stages are valid unless the consumer rejects them.** Generic composition never silently deduplicates repeated names or implementations.
11. **Adapters remain optional.** The contract is usable for HTTP, TCP, RPC, queues, tests, and application-specific request types.
12. **JavaScript runtime neutrality.** `generic.ts` must not import Node-only modules or branch on Node, Bun, or Deno globals. Runtime-specific adaptation belongs outside the generic core.
13. **Ambient context parity is executable.** TypeScript request context must preserve nested restoration, captured-context re-entry, async continuation, and concurrent-request isolation on every declared JavaScript runtime.

## Language mapping

| Semantic concept | Rust | TypeScript | Go | Gleam | Elixir | Erlang |
| --- | --- | --- | --- | --- | --- | --- |
| Consumer provider | `StaticAuthVerifier` / closure adapter | `Provider<Input, Output>` | `Provider[Input, Output]` | `Provider(input, output, error)` | unary function via `provider_from/1` | unary fun via `provider_from/1` |
| Contextual provider | typed request/provider state | `ContextualInput<Request, Context>` | `ContextualInput[Request, Metadata]` | `ContextualInput(request, context)` | `%{request:, context:}` | `#{request :=, context :=}` |
| Generic handler | typed stage/handler closures | `Handler<Request, Response>` | `GenericHandler[Request, Response]` | `Handler(request, response)` | unary function | arity-1 fun |
| Generic middleware | stage/layer abstractions | `Middleware<Request, Response>` | `GenericMiddleware[Request, Response]` | `Middleware(request, response)` | higher-order function | higher-order fun |
| Named middleware | composition plan/policy | `NamedMiddleware<Request, Response>` | `NamedGenericMiddleware[Request, Response]` | `NamedMiddleware(request, response)` | `%{name:, middleware:}` | `#{name :=, middleware :=}` |
| Composition | insertion/declaration order | `composeMiddleware` | `ComposeGeneric` | `compose` | `compose/2` | `compose/2` |
| Named composition | consumer order validation | `composeNamedMiddleware` | `ComposeNamedGeneric` | `compose_named` | `compose_named/2` | `compose_named/2` |
| Contextual composition | typed state path | `composeContextualMiddleware` | contextual handler path | contextual generic path | `compose_contextual/2` | `compose_contextual/2` |

The names are intentionally idiomatic rather than mechanically identical. Semantic parity is required; syntax parity is not.

## JavaScript runtime matrix

The TypeScript implementation is one language surface executed on three independently tested runtimes:

| Runtime | Minimum tested version | Required evidence |
| --- | --- | --- |
| Node.js | 22.23.1 | package build/tests, generic/provider semantics, `AsyncLocalStorage` context semantics, Fetch adapter smoke |
| Bun | 1.4.2 | generic/provider semantics, context semantics, `@oresoftware/ores-middleware/bun`, native `Bun.serve` loopback admission |
| Deno | 2.9.6 | generic/provider semantics, Node-compat context semantics, `@oresoftware/ores-middleware/deno`, native `Deno.serve` loopback admission |

`package.json` and `src/ts/package.json` carry identical `oresRuntimeSupport` metadata. That metadata is descriptive; CI execution is the evidence. A runtime is not supported merely because it parses the package or because another JavaScript runtime passed.

The root and publishable TypeScript export maps must remain semantically identical. `scripts/check-ts-package-exports.mjs` requires explicit `./bun` and `./deno` entrypoints in addition to the runtime-neutral `./generic`, `./context`, and `./adapters` boundaries.

The Bun/Deno modules use structural Fetch-compatible function types rather than importing Bun or Deno SDK typings. Listener ownership, TLS, websocket upgrades, permissions, and shutdown lifecycle remain consumer-owned.

## Provider version injection

A consumer can capture any concrete provider/version inside a closure and expose only the narrow generic interface. Different services or routes may therefore use different provider versions without making the provider SDK a transitive dependency of `ores-middleware`.

The generic layer avoids package-level singleton provider state and assumptions that there is one global auth implementation.

## Framework adapters

Framework-specific adapters are thin translation layers: parse framework request/context values, call the generic provider or middleware chain, and translate the result back into the framework response/error model. They must not reorder the consumer chain or force unrelated provider dependencies into the generic package.

## CI enforcement

`scripts/check-generic-portability.mjs` checks all six first-class language cores for the required generic concepts and rejects common concrete provider SDK names. It also rejects selected framework bindings from generic cores and Node/Bun/Deno-specific bindings from `generic.ts`.

`.github/workflows/generic-portability.yml` executes focused native conformance in Rust, TypeScript, Go, Gleam, Elixir, and Erlang. Source-shape checks are corroborating evidence, not a substitute for runtime execution.

`.github/workflows/js-runtime-portability.yml` installs exact Node, Bun, and Deno versions, builds the exact TypeScript package, executes the same generic/provider/context smoke suite under all three runtimes, compares normalized witnesses, and emits source-bound receipts. The native Fetch adapter matrix separately exercises actual Bun and Deno loopback servers, request-contract rejection paths, explicit runtime entrypoints, context isolation, and concurrent requests.

The repository-wide contract-conformance and generated-runtime gates continue to verify descriptors/data contracts across all six languages. Independent `*-test` consumer repositories may pin immutable `ores-middleware` commits to validate package/module consumption outside this repository.
