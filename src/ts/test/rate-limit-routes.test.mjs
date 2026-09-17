import assert from "node:assert/strict";
import test from "node:test";

import {
  RouteRateLimitResolutionError,
  resolveRouteRateLimitPolicy,
  validateRouteRateLimitTable
} from "../dist/rate-limit-routes.js";

test("different routes select different policies", () => {
  const table = {
    default_policy: { id: "default", capacity: 100 },
    routes: [
      {
        selector: { methods: ["GET"], path_template: "/search" },
        policy: { id: "search", capacity: 120 }
      },
      {
        selector: {
          methods: ["POST"],
          path_template: "/auth/login",
          operation_id: "auth.login"
        },
        policy: { id: "login", capacity: 8 }
      }
    ]
  };

  assert.deepEqual(
    resolveRouteRateLimitPolicy(table, {
      method: "POST",
      path: "/auth/login",
      route_template: "/auth/login",
      operation_id: "auth.login"
    }),
    { policy: { id: "login", capacity: 8 }, source: "route" }
  );

  assert.deepEqual(
    resolveRouteRateLimitPolicy(table, {
      method: "GET",
      path: "/not-overridden"
    }),
    { policy: { id: "default", capacity: 100 }, source: "default" }
  );
});

test("parameterized path templates match concrete paths", () => {
  const table = {
    routes: [{
      selector: {
        methods: ["GET"],
        path_template: "/ledger/{ledger_id}/entries/{entry_id}"
      },
      policy: { id: "ledger-read" }
    }]
  };

  assert.equal(
    resolveRouteRateLimitPolicy(table, {
      method: "GET",
      path: "/ledger/abc/entries/42"
    })?.policy.id,
    "ledger-read"
  );
});

test("operation id wins over a less-specific path rule", () => {
  const table = {
    routes: [
      {
        selector: { methods: ["POST"], path_template: "/jobs/{job_id}" },
        policy: { id: "job-path" }
      },
      {
        selector: { methods: ["POST"], operation_id: "jobs.retry" },
        policy: { id: "job-retry" }
      }
    ]
  };

  assert.equal(
    resolveRouteRateLimitPolicy(table, {
      method: "POST",
      path: "/jobs/123",
      route_template: "/jobs/{job_id}",
      operation_id: "jobs.retry"
    })?.policy.id,
    "job-retry"
  );
});

test("ambiguous equal-specificity rules fail instead of selecting by order", () => {
  const selector = { methods: ["GET"], path_template: "/users/{id}" };
  const table = {
    routes: [
      { selector, policy: { id: "users-a" } },
      { selector, policy: { id: "users-b" } }
    ]
  };

  assert.throws(
    () => resolveRouteRateLimitPolicy(table, { method: "GET", path: "/users/123" }),
    RouteRateLimitResolutionError
  );
  assert.ok(validateRouteRateLimitTable(table).some((issue) => issue.code === "duplicate-route-selector"));
});
