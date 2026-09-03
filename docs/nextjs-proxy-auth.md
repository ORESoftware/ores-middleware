# Next.js proxy and paired browser authentication

This document records the reusable boundary ideas from the legacy
`dancing-dragons/dd-next-1/src/middleware.ts` implementation without carrying
forward its provider coupling. The source is a migration reference, not a code
dependency.

Next.js now calls this network-boundary file `proxy.ts`; older applications may
still call it `middleware.ts`. `@oresoftware/ores-middleware/nextjs` therefore
exports the modern `nextjsProxy` name and keeps `nextjsMiddleware` only as a
deprecated compatibility alias.

## Authority model

Browser authentication in the ORES ecosystem has three distinct roles:

```text
Supabase session adapter ---- refresh + normalized provider subject --+
                                                                     |
Neon Auth session adapter --- refresh + normalized provider subject --+--> shared-auth
                                                                            |
                                                                            +--> canonical ORES user/tenant
```

- **Supabase Auth** owns its provider session, cookie refresh, and a normalized
  provider-local subject.
- **Neon Auth** owns its provider session, cookie refresh, and a normalized
  provider-local subject.
- **`github.com/shared-auth` is the canonical authority.** It verifies the
  request, maps both provider subjects, and returns the ORES user and optional
  tenant. Provider-local subjects may differ; only Shared Auth owns their
  mapping.
- **Clerk is not part of this contract.** The type vocabulary accepts only
  `supabase` and `neon-auth`, runtime validation rejects any other provider, and
  caller-supplied `x-clerk-*` headers are stripped.

This paired browser-session helper is complementary to the existing
`sharedAuthHttpVerifier`, which remains appropriate for server/API bearer-token
introspection. Neither helper performs product-resource authorization; handlers
and authorization middleware must still enforce roles, membership, ownership,
and resource policy.

## What belongs in `proxy.ts`

The proxy may:

1. classify exact or prefix routes as ignored, public, anonymous-only,
   authenticated page, or authenticated API;
2. skip authentication for framework assets, health probes, and explicit
   Shared Auth ceremony/callback routes;
3. ask injected Supabase and Neon Auth ports to refresh browser sessions;
4. propagate the returned `Set-Cookie` values without parsing or logging them;
5. ask Shared Auth to bind both provider subjects to one canonical ORES identity;
6. redirect an anonymous page request to a same-origin sign-in path;
7. return a no-store `401` problem response for an anonymous protected API;
8. redirect an authenticated user away from an anonymous-only sign-in page; and
9. pass a sanitized request downstream with only canonical internal identity
   headers.

The proxy must not:

- import Clerk or accept Clerk as an alternate authority;
- trust provider SDK output as the canonical application identity;
- query application tables to decide identity;
- run slow product data fetching;
- forward provider-local subjects, tokens, arbitrary claims, or cookies in
  request headers;
- reflect internal identity headers to the browser; or
- replace downstream authorization.

## Fail-closed behavior

The implementation rejects these states before the application handler runs:

- only one of the two provider sessions is authenticated;
- a provider resolver reports the wrong provider name;
- a provider emits malformed or response-splitting cookie data;
- Shared Auth is unavailable or returns a malformed decision;
- Shared Auth reports an authenticated browser identity without both provider
  sessions;
- Shared Auth rejects paired evidence; or
- Shared Auth bindings do not exactly match the refreshed provider subjects.

Provider and Shared Auth exception text is not returned to the caller. Auth
responses use `Cache-Control: no-store` and vary on `Authorization` and `Cookie`.

## Header boundary

Before any provider or application code sees the request, the helper removes:

- every `x-ores-auth-*` header;
- every `x-clerk-*` header;
- every `x-supabase-auth-*` header;
- every `x-neon-auth-*` header; and
- common unscoped identity headers such as `x-user-id` and `x-tenant-id`.

After Shared Auth succeeds, the helper adds only:

```text
x-ores-auth-authority: shared-auth
x-ores-auth-user-id: <canonical ORES user>
x-ores-auth-tenant-id: <canonical ORES tenant, when present>
x-ores-auth-evidence: supabase,neon-auth
```

The provider-local subjects remain in the in-process verification result and
are never forwarded to the handler.

## Package API

```ts
import {
  createPairedAuthProxy,
  defaultPairedAuthProxyPolicy,
} from "@oresoftware/ores-middleware/nextjs";

const evaluateAuth = createPairedAuthProxy({
  policy: defaultPairedAuthProxyPolicy(),
  dependencies: {
    async resolveSupabaseSession(request) {
      // Adapt @supabase/ssr here. Return a verified provider-local subject and
      // opaque refreshed Set-Cookie values; never return an access token.
      return { provider: "supabase" };
    },
    async resolveNeonAuthSession(request) {
      // Adapt the Neon Auth server SDK here under the same narrow contract.
      return { provider: "neon-auth" };
    },
    async verifyWithSharedAuth(request, sessions) {
      // Call or embed github.com/shared-auth. It must independently verify the
      // request and bind sessions.supabase/session.neonAuth before returning an
      // ORES user/tenant.
      return { kind: "anonymous" };
    },
  },
});
```

A Next.js application can translate the provider-neutral result at its final
framework edge:

```ts
import { NextResponse, type NextRequest } from "next/server";

export async function proxy(request: NextRequest) {
  const result = await evaluateAuth(request);
  if (result.kind === "response") return result.response;

  const response = NextResponse.next({
    request: { headers: result.request.headers },
  });
  for (const value of result.setCookieHeaders) {
    response.headers.append("set-cookie", value);
  }
  return response;
}
```

`Set-Cookie` values are intentionally opaque because parsing and reconstructing
provider cookies can drop security attributes or combine cookies incorrectly.
Applications must never log these values.

## Route semantics

Rules are evaluated in declaration order. Exact rules match one pathname;
prefix rules match a whole path segment, so `/admin` does not match
`/administrator`. The secure default protects every unmatched dynamic route.

The default policy ignores `/_next`, favicon/robots/sitemap assets, `/healthz`,
`/readyz`, and `/api/auth`. The last path is reserved for explicit Shared Auth
ceremonies and provider callbacks, where a partially established login must be
completed by the ceremony handler rather than mistaken for a valid application
session.

Public dynamic routes still refresh both sessions and consult Shared Auth. This
prevents a personalized public page from silently treating an authenticated or
denied user as anonymous during a provider outage. Routes that must remain
independent of the auth plane should be classified as `ignore` and must not use
identity-dependent behavior.
