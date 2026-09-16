import assert from "node:assert/strict";
import test from "node:test";

import { createMiddleware, defaultConfig } from "../dist/index.js";

function config() {
  const value = defaultConfig("auth-provider-boundary-test");
  value.environment = "test";
  value.settings.tls.requireHttps = false;
  value.settings.rateLimit.enabled = false;
  value.settings.idempotency.enabled = false;
  value.settings.compression.enabled = false;
  return value;
}

function request() {
  return new Request("http://example.test/me", {
    headers: { accept: "application/json" }
  });
}

test("provider claims never become ambient baggage implicitly", async () => {
  let observed;
  const middleware = createMiddleware(config(), {
    authVerifier: async () => ({
      userId: "alice",
      tenantId: "tenant-a",
      claims: {
        "otel.secret": "must-not-propagate",
        role: "admin"
      }
    }),
    telemetry: {
      started(context) { observed = context; },
      finished() {}
    }
  });

  const response = await middleware(request(), async () => new Response(null, { status: 204 }));
  assert.equal(response.status, 204);
  assert.equal(observed.userId, "alice");
  assert.equal(observed.tenantId, "tenant-a");
  assert.deepEqual(observed.baggage, {});
});

test("consumer can explicitly allow-list provider data into baggage", async () => {
  let observed;
  const middleware = createMiddleware(config(), {
    authVerifier: async () => ({
      userId: "alice",
      tenantId: "tenant-a",
      claims: {
        "auth.assurance": "mfa",
        "otel.secret": "must-not-propagate"
      }
    }),
    authBaggageEnricher: (_request, _context, auth) => ({
      "auth.assurance": auth.claims?.["auth.assurance"] ?? "unknown"
    }),
    telemetry: {
      started(context) { observed = context; },
      finished() {}
    }
  });

  const response = await middleware(request(), async () => new Response(null, { status: 204 }));
  assert.equal(response.status, 204);
  assert.deepEqual(observed.baggage, { "auth.assurance": "mfa" });
  assert.equal(observed.baggage["otel.secret"], undefined);
});
