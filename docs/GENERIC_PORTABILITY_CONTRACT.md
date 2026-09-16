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

## Language mapping

| Semantic concept | Rust | TypeScript | Go | Gleam |
| --- | --- | --- | --- | --- |
| Consumer provider | `AuthProvider` / closure adapter / typed verifier | `Provider<Input, Output>` | `Provider[Input, Output]` | `Provider(input, output, error)` |
| Contextual provider | typed request/provider state | `ContextualInput<Request, Context>` | `ContextualInput[Request, Metadata]` | `ContextualInput(request, context)` |
| Generic handler | stage/handler traits and typed closures | `Handler<Request, Response>` | `GenericHandler[Request, Response]` | `Handler(request, response)` |
| Generic middleware | stage/layer abstractions | `Middleware<Request, Response>` | `GenericMiddleware[Request, Response]` | `Middleware(request, response)` |
| Named middleware | consumer order policy / stage metadata | `NamedMiddleware<Request, Response>` | `NamedGenericMiddleware[Request, Response]` | `NamedMiddleware(request, response)` |
| Composition | insertion/declaration order | `composeMiddleware` | `ComposeGeneric` | `compose` |
| Named composition | consumer-authored rules | `composeNamedMiddleware` | `ComposeNamedGeneric` | `compose_named` |

The names above are intentionally idiomatic rather than mechanically identical. Semantic parity is required; syntax parity is not.

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

`scripts/check-generic-portability.mjs` statically checks the TypeScript, Go, and Gleam generic modules for the required concepts and rejects common concrete provider SDK names in those modules. Runtime language tests separately verify order, context, failure propagation, empty chains, and duplicate-name behavior.

Independent `*-test` consumer repositories pin immutable `ores-middleware` commits and consume the public language/package boundary so packaging/module regressions are caught outside this repository.
