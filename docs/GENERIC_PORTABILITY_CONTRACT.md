# Generic portability contract

The generic middleware surface is a cross-language contract for consumers that do **not** want ORES to choose their HTTP framework, auth SDK, middleware order, request type, response type, or failure model.

Rust, TypeScript, Go, and Gleam may use different language mechanisms, but the semantic contract is the same:

1. **Consumer-owned providers.** The consumer owns the concrete SDK, exact dependency version/revision, credentials, lifecycle, and retry behavior. `ores-middleware` receives only a narrow provider/verification function or interface.
2. **Generic request/response types.** Generic middleware is parameterized over request/response/context types instead of requiring an ORES request model or one HTTP framework.
3. **Consumer-owned order.** Composition preserves declaration order exactly. The library does not infer that `auth`, `rate-limit`, `request-id`, or any other named stage must run first.
4. **Opaque names.** Named middleware metadata is preserved for consumer validation/observability but is not interpreted, deduplicated, sorted, or normalized by the generic composition layer.
5. **Consumer-owned failure semantics.** TypeScript can use exceptions or typed result unions; Go uses `error`; Gleam commonly embeds `Result` in the provider or response type; Rust uses typed `Result`/provider errors. The generic layer must not collapse these into a provider-specific error type.
6. **Context stays owned by the caller.** Go propagates `context.Context`; TypeScript contextual middleware receives the consumer's context shape; Gleam's generic response/request types can carry process/effect boundaries; Rust provider/composition types retain concrete request/provider state.
7. **No provider SDK dependencies in the generic core.** The generic modules must not import or depend on Supabase, Clerk, Auth0, Firebase, Okta, Cognito, or another concrete identity provider. Framework/provider adapters belong outside the generic core.
8. **Identity chains are valid.** Empty middleware lists preserve the handler unchanged/semantically unchanged.
9. **Duplicate stages are valid unless the consumer rejects them.** Generic composition does not silently deduplicate repeated middleware names or implementations.
10. **Adapters remain optional.** HTTP/framework adapters may translate into the generic contract, but the contract itself must remain usable for HTTP, TCP, RPC, queues, tests, or application-specific request types.
11. **JavaScript runtime neutrality.** The TypeScript generic core must not import Node-only modules or branch on Node, Bun, or Deno globals. Runtime-specific adaptation belongs outside `generic.ts`.
12. **Ambient context parity is executable.** The TypeScript request-context carrier must preserve nested restoration, captured-context re-entry, async continuation, and concurrent-request isolation on every declared JavaScript runtime.

## Language mapping

| Semantic concept | Rust | TypeScript | Go | Gleam |
| --- | --- | --- | --- | --- |
| Consumer provider | `StaticAuthVerifier` / closure adapter / explicit `dyn` bridge | `Provider<Input, Output>` | `Provider[Input, Output]` | `Provider(input, output, error)` |
| Contextual provider | typed request/provider state | `ContextualInput<Request, Context>` | `ContextualInput[Request, Metadata]` | `ContextualInput(request, context)` |
| Generic handler | typed stage/handler closures | `Handler<Request, Response>` | `GenericHandler[Request, Response]` | `Handler(request, response)` |
| Generic middleware | stage/layer abstractions | `Middleware<Request, Response>` | `GenericMiddleware[Request, Response]` | `Middleware(request, response)` |
| Named middleware | `MiddlewareCompositionPlan` / consumer policy | `NamedMiddleware<Request, Response>` | `NamedGenericMiddleware[Request, Response]` | `NamedMiddleware(request, response)` |
| Composition | insertion/declaration order | `composeMiddleware` | `ComposeGeneric` | `compose` |
| Named composition | `validate_consumer_middleware_order` | `composeNamedMiddleware` | `ComposeNamedGeneric` | `compose_named` |

The names above are intentionally idiomatic rather than mechanically identical. Semantic parity is required; syntax parity is not.

## JavaScript runtime matrix

The TypeScript implementation is one language surface executed on three independently tested runtimes:

| Runtime | Minimum tested version | Required evidence |
| --- | --- | --- |
| Node.js | 22.23.1 | package build/tests, generic/provider semantics, `AsyncLocalStorage` context semantics, Fetch adapter smoke |
| Bun | 1.4.2 | generic/provider semantics, `AsyncLocalStorage` context semantics, native `Bun.serve` loopback Fetch admission |
| Deno | 2.9.6 | generic/provider semantics, Node-compat `AsyncLocalStorage` context semantics, native `Deno.serve` loopback Fetch admission |

`package.json` and `src/ts/package.json` carry identical `oresRuntimeSupport` metadata. That metadata is descriptive; CI execution is the evidence. A runtime is not considered supported merely because it can parse the package or because another JavaScript runtime passed.

The root package export map and the publishable `src/ts` export map must also remain semantically identical. `scripts/check-ts-package-exports.mjs` enforces the mapping so a subpath such as `./generic`, `./context`, or `./adapters` cannot silently exist in only one package boundary.

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

`scripts/check-generic-portability.mjs` checks Rust, TypeScript, Go, and Gleam for the required generic concepts and rejects common concrete provider SDK names from those cores. It additionally rejects Node/Bun/Deno-specific bindings from the TypeScript generic core.

`.github/workflows/generic-portability.yml` executes focused native conformance in all four generic-core languages rather than treating source-shape checks as sufficient evidence. The broader reproducibility workflows continue to run their full language suites on their supported operating-system matrices.

`.github/workflows/js-runtime-portability.yml` installs exact Node, Bun, and Deno versions, builds the exact TypeScript package, executes the same generic/provider/context smoke suite under all three runtimes, compares normalized witnesses to the Node baseline, and emits a source-bound receipt. The separate native Fetch adapter matrix exercises actual Bun and Deno loopback servers, request-contract rejection paths, context isolation, and concurrent requests.

The repository-wide `contract-conformance` and generated-runtime gates continue to cover Rust, TypeScript, Go, Gleam, Elixir, and Erlang descriptors/data contracts. Generic provider injection is currently a four-language contract; Elixir and Erlang remain part of the wider middleware conformance surface without pretending they expose this exact generic API.

Independent `*-test` consumer repositories pin immutable `ores-middleware` commits and consume the public language/package boundary so packaging/module regressions are caught outside this repository.
