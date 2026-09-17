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
| Named middleware | composition plan/policy | `NamedMiddleware<Request, Response>` | `NamedGenericMiddleware[Request, Response]` | `NamedMiddleware(request, response)` | `%{name:, middleware:}` | map with `name` and `middleware` |
| Composition | insertion/declaration order | `composeMiddleware` | `ComposeGeneric` | `compose` | `compose/2` | `compose/2` |
| Named composition | consumer policy | `composeNamedMiddleware` | `ComposeNamedGeneric` | `compose_named` | `compose_named/2` | `compose_named/2` |
| Contextual composition | typed request/provider state | contextual middleware | contextual generic middleware | caller-carried context | `compose_contextual/2` | `compose_contextual/2` |

The names above are intentionally idiomatic rather than mechanically identical. Semantic parity is required; syntax parity is not.

## JavaScript runtime matrix

The TypeScript implementation is one language surface executed on three independently tested runtimes:

| Runtime | Minimum tested version | Required evidence |
| --- | --- | --- |
| Node.js | 22.23.1 | package build/tests, generic/provider semantics, `AsyncLocalStorage` context semantics, Fetch adapter smoke |
| Bun | 1.4.2 | generic/provider semantics, `AsyncLocalStorage` context semantics, native `Bun.serve` loopback Fetch admission |
| Deno | 2.9.6 | generic/provider semantics, Node-compat `AsyncLocalStorage` context semantics, native `Deno.serve` loopback Fetch admission |

`package.json` and `src/ts/package.json` carry identical `oresRuntimeSupport` metadata. That metadata is descriptive; CI execution is the evidence. A runtime is not considered supported merely because it can parse the package or because another JavaScript runtime passed.

The root package export map and the publishable `src/ts` export map must also remain semantically identical. `scripts/check-ts-package-exports.mjs` enforces the mapping so subpaths such as `./generic`, `./context`, `./adapters`, `./bun`, and `./deno` cannot silently exist in only one package boundary.

## Provider version injection

A consumer can capture any concrete provider/version inside a closure and expose only the narrow generic interface. That permits two services—or even two routes in one process—to use different provider versions without making the provider SDK a transitive dependency of `ores-middleware`.

The generic layer must therefore avoid package-level singleton provider state and avoid assumptions that there is one global auth implementation.

## Framework adapters

Framework-specific adapters should be thin translation layers:

- parse framework request/context values;
- call the generic provider or generic middleware chain;
- translate the generic result back into the framework response/error model.

They must not reorder the consumer's middleware chain or force unrelated provider dependencies into the generic package.

## CI enforcement

`scripts/check-generic-portability.mjs` checks Rust, TypeScript, Go, Gleam, Elixir, and Erlang for their required generic concepts and rejects concrete provider SDK names from those cores. It additionally rejects framework bindings from the generic cores where applicable and Node/Bun/Deno-specific bindings from the TypeScript generic core.

`.github/workflows/generic-portability.yml` executes focused native conformance in all six generic-core languages rather than treating source-shape checks as sufficient evidence. Each lane checks out and verifies the exact candidate SHA. The broader reproducibility workflows continue to run their full language suites on their supported operating-system matrices.

`.github/workflows/js-runtime-portability.yml` installs exact Node, Bun, and Deno versions, builds the exact TypeScript package, executes the same generic/provider/context smoke suite under all three runtimes, compares normalized witnesses to the Node baseline, and emits a source-bound receipt. The separate native Fetch adapter matrix exercises actual Bun and Deno loopback servers, request-contract rejection paths, context isolation, and concurrent requests.

The repository-wide `contract-conformance` and generated-runtime gates continue to cover the wider middleware descriptors/data contracts. The six-language generic portability gate is the narrower executable proof that provider injection, caller-owned context, composition order, duplicate-stage preservation, and framework/provider neutrality remain semantically aligned.

Independent `*-test` consumer repositories pin immutable `ores-middleware` commits and consume the public language/package boundary so packaging/module regressions are caught outside this repository.
