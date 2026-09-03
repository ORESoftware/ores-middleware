# Framework adapter rollout

The core decision functions in this repository deliberately avoid framework
request/response types. Thin adapters should be landed in separate, reviewable
pull requests after the core contract is green.

| Language core | Intended adapters | Rule |
| --- | --- | --- |
| Rust | Tower, Axum, MASH/Maud+Axum+HTMX, Leptos, Dioxus | Extract normalized fields; do not duplicate selection logic |
| TypeScript/JavaScript | Express, Hono, Bun, Deno, NestJS, Next.js, Nuxt, Hapi | Use Web/Node adapters around the same `decideDocs` function |
| Go | `net/http`, Gorilla Mux, Gin, Echo, Fiber | Translate request/response types only |
| Gleam | Wisp/Mist and OTP-compatible handlers | Keep supervision/lifecycle outside the selector |
| Elixir | Plug/Phoenix | Delegate semantics to the same fixture-defined contract |
| Erlang | Cowboy | Preserve OTP ownership and fail-closed digest behavior |

## Next.js proxy boundary

Modern Next.js applications should import `nextjsProxy` from
`@oresoftware/ores-middleware/nextjs` and place the application adapter in
`proxy.ts`. The same subpath retains `nextjsMiddleware` as a deprecated source
compatibility alias for applications that still use the older filename.

The proxy is a thin network boundary, not an identity database or authorization
engine. Route classification, same-origin redirects, caller-header sanitation,
and refreshed cookie propagation belong there. Canonical browser identity is
established by `shared-auth` only after it binds the paired Supabase and Neon
Auth evidence. Clerk is not an accepted provider or fallback. See
[`nextjs-proxy-auth.md`](./nextjs-proxy-auth.md).

An adapter is complete only when it:

1. passes the shared conformance corpus;
2. proves unknown paths call the next handler exactly once;
3. never logs or reflects authorization/cookie headers;
4. supports streaming or byte bodies supplied by an injected API-doc provider;
5. preserves `HEAD` semantics without loading or writing a body when the provider
   can avoid it; and
6. exposes exact contract and implementation versions in its package metadata.
