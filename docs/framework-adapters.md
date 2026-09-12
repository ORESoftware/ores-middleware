# Framework adapter rollout

The core decision functions in this repository deliberately avoid framework
request/response types. Thin adapters should be landed in separate, reviewable
pull requests after the core contract is green.

The design rule is **one runtime-native core, thin framework boundaries**. Do
not reimplement request identity, auth, rate limiting, deadlines, telemetry,
contract validation, or response policy inside an Express/Gin/Axum/etc. shim.
If two frameworks share the same native request/response substrate, adapt that
substrate once and keep their wrappers mechanical.

| Language core | Supported / intended adapters | Rule |
| --- | --- | --- |
| Rust | Tower/`http`, Axum, MASH/Maud+Axum+HTMX, Leptos, Dioxus; add Actix/Rocket/Warp adapters only where their boundary cannot reuse the Tower/HTTP core | Extract normalized fields; do not duplicate selection logic |
| TypeScript/JavaScript | Fetch/Web core for Node.js, Bun, Deno, Hono, Next.js and Nuxt; Node bridges for Express, Koa, Fastify, NestJS and Hapi | Convert to/from `Request`/`Response`; preserve the native framework lifecycle and context |
| Go | `net/http` core, Gorilla Mux, Gin, Echo, Fiber | Prefer `http.Handler` composition; framework wrappers translate only the edge |
| Gleam | `gleam/http`, Wisp, Mist, Cowboy and OTP-compatible handlers | Keep supervision/lifecycle outside the policy engine |
| Elixir | Plug/Phoenix | Delegate semantics to the same fixture-defined contract |
| Erlang | Cowboy, Ranch, Elli and OTP | Preserve OTP ownership and fail-closed digest behavior |

## JavaScript runtime and framework boundaries

`PortableMiddleware` uses the WHATWG Fetch `Request`/`Response` contract. Bun,
Deno and Hono therefore need no separate policy implementation. Node-oriented
frameworks use thin lifecycle adapters:

- `expressMiddleware` for Express and compatible NestJS/Connect boundaries;
- `koaMiddleware` / `createKoaMiddleware` for Koa; install it after the body
  parser when parsed request bodies participate in request-contract validation;
- `fastifyPreHandler` / `fastifyMiddleware` for Fastify. The hook releases
  Fastify to run the route but keeps the portable middleware lifecycle open
  until the native response emits `finish`/`close`, so telemetry and deadlines
  do not incorrectly finish at pre-handler return;
- `hapi*`, Hono, Next.js, Nuxt, Bun and Deno adapters remain backed by the same
  portable core.

Adapters must not create a second auth/session authority and must not weaken
TJSV-backed request-contract admission.

## Go composition

The Go package exposes `Adapter`, `HandlerFuncAdapter`, `Chain`, `ChainFuncs`,
`AsAdapter`, and `HandlerFuncAdapterFromHandler`. This preserves declaration
order while composing in reverse around the final handler, mirroring the useful
middleware-chaining pattern in `ORESoftware/cp-go-api` without importing a
router into the core package.

The framework adapters for Gorilla Mux, Gin and Echo can all pass their native
routers through the `net/http` core. Fiber uses its official adaptor to cross
into `net/http`, after which the same stack applies.

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

1. passes the shared conformance corpus and its native framework integration tests;
2. proves a continued request calls the next/handler boundary exactly once;
3. proves a portable short-circuit does **not** invoke the downstream handler;
4. never logs or reflects authorization/cookie headers;
5. preserves framework request context for the real downstream lifecycle;
6. supports streaming or byte bodies where the framework/runtime can do so without unsafe buffering;
7. preserves `HEAD` semantics without loading or writing a body when the provider can avoid it; and
8. exposes exact contract and implementation versions in package metadata.
