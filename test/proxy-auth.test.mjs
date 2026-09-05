import assert from "node:assert/strict";
import test from "node:test";

import {
  createPairedAuthProxy,
  defaultPairedAuthProxyPolicy,
  nextjsMiddleware,
  nextjsProxy,
  proxyIdentityHeaders
} from "@oresoftware/ores-middleware/nextjs";

const anonymous = (provider, cookies = []) => ({ provider, setCookieHeaders: cookies });
const session = (provider, subject, cookies = [], tenantId) => ({
  provider,
  session: { subject, ...(tenantId ? { tenantId } : {}) },
  setCookieHeaders: cookies
});

function dependencies(overrides = {}) {
  return {
    resolveSupabaseSession: async () => anonymous("supabase", ["sb=refreshed; Path=/; HttpOnly"]),
    resolveNeonAuthSession: async () => anonymous("neon-auth", ["neon=refreshed; Path=/; HttpOnly"]),
    verifyWithSharedAuth: async () => ({ kind: "anonymous" }),
    ...overrides
  };
}

function proxy(overrides = {}) {
  return createPairedAuthProxy({
    policy: defaultPairedAuthProxyPolicy(),
    dependencies: dependencies(),
    ...overrides
  });
}

test("exports proxy.ts naming while retaining the old middleware alias", () => {
  assert.equal(nextjsProxy, nextjsMiddleware);
});

test("ignored framework assets skip auth and strip caller-controlled identity", async () => {
  let calls = 0;
  const evaluate = proxy({
    dependencies: dependencies({
      resolveSupabaseSession: async () => { calls += 1; return anonymous("supabase"); },
      resolveNeonAuthSession: async () => { calls += 1; return anonymous("neon-auth"); },
      verifyWithSharedAuth: async () => { calls += 1; return { kind: "anonymous" }; }
    })
  });
  const result = await evaluate(new Request("https://app.example/_next/static/a.js", {
    headers: {
      "x-clerk-user-id": "forged-clerk-user",
      "x-ores-auth-user-id": "forged-ores-user",
      "x-user-id": "forged-generic-user"
    }
  }));
  assert.equal(result.kind, "next");
  assert.equal(result.access, "ignore");
  assert.equal(calls, 0);
  assert.equal(result.request.headers.get("x-clerk-user-id"), null);
  assert.equal(result.request.headers.get("x-ores-auth-user-id"), null);
  assert.equal(result.request.headers.get("x-user-id"), null);
});

test("anonymous protected pages redirect safely and preserve both refresh cookies", async () => {
  const evaluate = proxy();
  const result = await evaluate(new Request("https://app.example/dashboard?tab=profile"));
  assert.equal(result.kind, "response");
  assert.equal(result.response.status, 307);
  const location = new URL(result.response.headers.get("location"));
  assert.equal(location.origin, "https://app.example");
  assert.equal(location.pathname, "/sign-in");
  assert.equal(location.searchParams.get("returnTo"), "/dashboard?tab=profile");
  assert.match(result.response.headers.get("set-cookie") ?? "", /sb=refreshed/);
  assert.match(result.response.headers.get("set-cookie") ?? "", /neon=refreshed/);
});

test("anonymous protected APIs receive a no-store problem response", async () => {
  const policy = {
    ...defaultPairedAuthProxyPolicy(),
    routes: [
      ...defaultPairedAuthProxyPolicy().routes,
      { path: "/api/private", match: "prefix", access: "authenticated-api" }
    ]
  };
  const result = await proxy({ policy })(new Request("https://app.example/api/private/items"));
  assert.equal(result.kind, "response");
  assert.equal(result.code, "authentication_required");
  assert.equal(result.response.status, 401);
  assert.equal(result.response.headers.get("cache-control"), "no-store");
  assert.equal((await result.response.json()).title, "authentication_required");
});

test("paired provider evidence is mapped only through shared-auth canonical identity", async () => {
  const evaluate = proxy({
    dependencies: dependencies({
      resolveSupabaseSession: async () => session("supabase", "supabase-subject", ["sb=fresh; Path=/"], "supabase-tenant"),
      resolveNeonAuthSession: async () => session("neon-auth", "neon-subject", ["neon=fresh; Path=/"], "neon-tenant"),
      verifyWithSharedAuth: async (_request, sessions) => {
        assert.equal(sessions.supabase.session.subject, "supabase-subject");
        assert.equal(sessions.neonAuth.session.subject, "neon-subject");
        return {
          kind: "authenticated",
          userId: "ores-user-1",
          tenantId: "ores-tenant-1",
          bindings: {
            supabase: "supabase-subject",
            "neon-auth": "neon-subject"
          }
        };
      }
    })
  });
  const result = await evaluate(new Request("https://app.example/dashboard", {
    headers: {
      "x-clerk-user-id": "forged",
      "x-supabase-auth-user": "also-forged"
    }
  }));
  assert.equal(result.kind, "next");
  assert.equal(result.identity.authority, "shared-auth");
  assert.equal(result.request.headers.get(proxyIdentityHeaders.authority), "shared-auth");
  assert.equal(result.request.headers.get(proxyIdentityHeaders.userId), "ores-user-1");
  assert.equal(result.request.headers.get(proxyIdentityHeaders.tenantId), "ores-tenant-1");
  assert.equal(result.request.headers.get(proxyIdentityHeaders.evidence), "supabase,neon-auth");
  assert.equal(result.request.headers.get("x-clerk-user-id"), null);
  assert.equal(result.request.headers.get("x-supabase-auth-user"), null);
  assert.equal(result.request.headers.get("x-provider-subject"), null);
  assert.deepEqual(result.setCookieHeaders, ["sb=fresh; Path=/", "neon=fresh; Path=/"]);
});

test("a one-sided provider session fails closed before shared-auth", async () => {
  let sharedCalls = 0;
  const evaluate = proxy({
    dependencies: dependencies({
      resolveSupabaseSession: async () => session("supabase", "supabase-subject"),
      resolveNeonAuthSession: async () => anonymous("neon-auth"),
      verifyWithSharedAuth: async () => { sharedCalls += 1; return { kind: "anonymous" }; }
    })
  });
  const result = await evaluate(new Request("https://app.example/dashboard"));
  assert.equal(result.kind, "response");
  assert.equal(result.code, "auth_pair_incomplete");
  assert.equal(result.response.status, 401);
  assert.equal(sharedCalls, 0);
});

test("shared-auth must bind the exact provider subjects", async () => {
  const evaluate = proxy({
    dependencies: dependencies({
      resolveSupabaseSession: async () => session("supabase", "supabase-subject"),
      resolveNeonAuthSession: async () => session("neon-auth", "neon-subject"),
      verifyWithSharedAuth: async () => ({
        kind: "authenticated",
        userId: "ores-user-1",
        bindings: { supabase: "wrong", "neon-auth": "neon-subject" }
      })
    })
  });
  const result = await evaluate(new Request("https://app.example/dashboard"));
  assert.equal(result.kind, "response");
  assert.equal(result.code, "auth_evidence_mismatch");
  assert.equal(result.response.status, 401);
});

test("Clerk evidence is rejected by the provider contract", async () => {
  const evaluate = proxy({
    dependencies: dependencies({
      resolveSupabaseSession: async () => ({
        provider: "clerk",
        session: { subject: "clerk-user" }
      })
    })
  });
  const result = await evaluate(new Request("https://app.example/dashboard"));
  assert.equal(result.kind, "response");
  assert.equal(result.code, "auth_provider_contract_violation");
  assert.equal(result.response.status, 503);
});

test("provider failures fail closed without exposing exception text", async () => {
  const evaluate = proxy({
    dependencies: dependencies({
      resolveNeonAuthSession: async () => { throw new Error("secret backend detail"); }
    })
  });
  const result = await evaluate(new Request("https://app.example/public"));
  assert.equal(result.kind, "response");
  assert.equal(result.code, "auth_provider_unavailable");
  const body = await result.response.text();
  assert.doesNotMatch(body, /secret backend detail/);
  assert.match(result.response.headers.get("set-cookie") ?? "", /sb=refreshed/);
});

test("authenticated users are redirected away from anonymous-only routes", async () => {
  const evaluate = proxy({
    dependencies: dependencies({
      resolveSupabaseSession: async () => session("supabase", "supabase-subject"),
      resolveNeonAuthSession: async () => session("neon-auth", "neon-subject"),
      verifyWithSharedAuth: async () => ({
        kind: "authenticated",
        userId: "ores-user-1",
        bindings: {
          supabase: "supabase-subject",
          "neon-auth": "neon-subject"
        }
      })
    })
  });
  const result = await evaluate(new Request("https://app.example/sign-in"));
  assert.equal(result.kind, "response");
  assert.equal(result.code, "already_authenticated");
  assert.equal(new URL(result.response.headers.get("location")).pathname, "/app");
});

test("response splitting in refresh cookies is a contract violation", async () => {
  const evaluate = proxy({
    dependencies: dependencies({
      resolveSupabaseSession: async () => anonymous("supabase", ["sb=ok\r\nx-forged: yes"])
    })
  });
  const result = await evaluate(new Request("https://app.example/dashboard"));
  assert.equal(result.kind, "response");
  assert.equal(result.code, "auth_provider_contract_violation");
});

test("public dynamic routes still refresh paired sessions and consult shared-auth", async () => {
  let sharedCalls = 0;
  const defaults = defaultPairedAuthProxyPolicy();
  const policy = {
    ...defaults,
    routes: [
      ...defaults.routes,
      { path: "/public", match: "prefix", access: "public" }
    ]
  };
  const evaluate = proxy({
    policy,
    dependencies: dependencies({
      verifyWithSharedAuth: async () => {
        sharedCalls += 1;
        return { kind: "anonymous" };
      }
    })
  });
  const result = await evaluate(new Request("https://app.example/public/about"));
  assert.equal(result.kind, "next");
  assert.equal(result.access, "public");
  assert.equal(sharedCalls, 1);
  assert.deepEqual(result.setCookieHeaders, [
    "sb=refreshed; Path=/; HttpOnly",
    "neon=refreshed; Path=/; HttpOnly"
  ]);
});

test("health probes use the ignore boundary and do not depend on auth providers", async () => {
  let calls = 0;
  const evaluate = proxy({
    dependencies: dependencies({
      resolveSupabaseSession: async () => { calls += 1; throw new Error("offline"); },
      resolveNeonAuthSession: async () => { calls += 1; throw new Error("offline"); },
      verifyWithSharedAuth: async () => { calls += 1; throw new Error("offline"); }
    })
  });
  const result = await evaluate(new Request("https://app.example/healthz"));
  assert.equal(result.kind, "next");
  assert.equal(result.access, "ignore");
  assert.equal(calls, 0);
});

test("shared-auth cannot establish a browser identity without both provider sessions", async () => {
  const evaluate = proxy({
    dependencies: dependencies({
      verifyWithSharedAuth: async () => ({
        kind: "authenticated",
        userId: "ores-user-1",
        bindings: { supabase: "missing", "neon-auth": "missing" }
      })
    })
  });
  const result = await evaluate(new Request("https://app.example/dashboard"));
  assert.equal(result.kind, "response");
  assert.equal(result.code, "auth_pair_incomplete");
  assert.equal(result.response.status, 401);
});

test("shared-auth denial is authoritative even on public routes", async () => {
  const defaults = defaultPairedAuthProxyPolicy();
  const policy = {
    ...defaults,
    routes: [...defaults.routes, { path: "/public", match: "prefix", access: "public" }]
  };
  const evaluate = proxy({
    policy,
    dependencies: dependencies({
      verifyWithSharedAuth: async () => ({ kind: "denied", status: 403, code: "account-suspended" })
    })
  });
  const result = await evaluate(new Request("https://app.example/public"));
  assert.equal(result.kind, "response");
  assert.equal(result.code, "shared_auth_denied");
  assert.equal(result.response.status, 403);
});

test("configuration rejects external redirects and duplicate route rules", () => {
  const defaults = defaultPairedAuthProxyPolicy();
  assert.throws(
    () => proxy({ policy: { ...defaults, signInPath: "https://evil.example/sign-in" } }),
    /same-origin path/
  );
  assert.throws(
    () => proxy({
      policy: {
        ...defaults,
        routes: [
          { path: "/same", match: "exact", access: "public" },
          { path: "/same", match: "exact", access: "authenticated-page" }
        ]
      }
    }),
    /duplicate proxy route rule/
  );
});

test("configuration requires all three auth ports", () => {
  const deps = dependencies();
  delete deps.verifyWithSharedAuth;
  assert.throws(
    () => proxy({ dependencies: deps }),
    /verifyWithSharedAuth must be a function/
  );
});
