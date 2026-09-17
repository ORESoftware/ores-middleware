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

// Issue #188/#191 adversarial matrix: 15 independent semantic checks.
test("exact static route beats a parameterized route", () => {
  const table = {
    routes: [
      { selector: { methods: ["GET"], path_template: "/users/{id}" }, policy: { id: "user-by-id" } },
      { selector: { methods: ["GET"], path_template: "/users/me" }, policy: { id: "current-user" } }
    ]
  };
  assert.equal(resolveRouteRateLimitPolicy(table, { method: "GET", path: "/users/me" })?.policy.id, "current-user");
});

test("wrong method falls back to the default policy", () => {
  const table = {
    default_policy: { id: "default" },
    routes: [{ selector: { methods: ["POST"], path_template: "/search" }, policy: { id: "search-post" } }]
  };
  assert.equal(resolveRouteRateLimitPolicy(table, { method: "GET", path: "/search" })?.policy.id, "default");
});

test("request method matching is case-insensitive", () => {
  const table = {
    routes: [{ selector: { methods: ["POST"], path_template: "/submit" }, policy: { id: "submit" } }]
  };
  assert.equal(resolveRouteRateLimitPolicy(table, { method: "post", path: "/submit" })?.policy.id, "submit");
});

test("no route and no default returns undefined", () => {
  const table = {
    routes: [{ selector: { methods: ["GET"], path_template: "/known" }, policy: { id: "known" } }]
  };
  assert.equal(resolveRouteRateLimitPolicy(table, { method: "GET", path: "/unknown" }), undefined);
});

test("query strings do not participate in path-template matching", () => {
  const table = {
    routes: [{ selector: { methods: ["GET"], path_template: "/search" }, policy: { id: "search" } }]
  };
  assert.equal(resolveRouteRateLimitPolicy(table, { method: "GET", path: "/search?q=ores" })?.policy.id, "search");
});

test("trusted registered route template can identify the matched route", () => {
  const table = {
    routes: [{ selector: { methods: ["GET"], path_template: "/teams/{team_id}" }, policy: { id: "team-read" } }]
  };
  assert.equal(
    resolveRouteRateLimitPolicy(table, {
      method: "GET",
      path: "/already-normalized-by-router",
      route_template: "/teams/{team_id}"
    })?.policy.id,
    "team-read"
  );
});

test("terminal catch-all matches deeper paths", () => {
  const table = {
    routes: [{ selector: { methods: ["GET"], path_template: "/assets/*" }, policy: { id: "assets" } }]
  };
  assert.equal(resolveRouteRateLimitPolicy(table, { method: "GET", path: "/assets/js/app.js" })?.policy.id, "assets");
});

test("nonterminal catch-all is rejected", () => {
  const table = {
    routes: [{ selector: { methods: ["GET"], path_template: "/assets/*/raw" }, policy: { id: "bad" } }]
  };
  assert.ok(validateRouteRateLimitTable(table).some((issue) => issue.code === "catch-all-must-be-terminal"));
});

test("trailing-slash route templates are rejected as non-canonical", () => {
  const table = {
    routes: [{ selector: { methods: ["GET"], path_template: "/search/" }, policy: { id: "bad" } }]
  };
  assert.ok(validateRouteRateLimitTable(table).some((issue) => issue.code === "non-canonical-path-template"));
});

test("empty route selectors are rejected", () => {
  const table = { routes: [{ selector: {}, policy: { id: "bad" } }] };
  assert.ok(validateRouteRateLimitTable(table).some((issue) => issue.code === "empty-route-selector"));
});

test("duplicate configured methods are rejected", () => {
  const table = {
    routes: [{ selector: { methods: ["GET", "GET"], path_template: "/search" }, policy: { id: "bad" } }]
  };
  assert.ok(validateRouteRateLimitTable(table).some((issue) => issue.code === "duplicate-http-method"));
});

test("lowercase configured HTTP methods are rejected", () => {
  const table = {
    routes: [{ selector: { methods: ["get"], path_template: "/search" }, policy: { id: "bad" } }]
  };
  assert.ok(validateRouteRateLimitTable(table).some((issue) => issue.code === "invalid-http-method"));
});

test("invalid operation IDs are rejected", () => {
  const table = {
    routes: [{ selector: { operation_id: "bad operation id" }, policy: { id: "bad" } }]
  };
  assert.ok(validateRouteRateLimitTable(table).some((issue) => issue.code === "invalid-operation-id"));
});

test("duplicate selector detection treats methods as an unordered set", () => {
  const table = {
    routes: [
      { selector: { methods: ["GET", "POST"], path_template: "/search" }, policy: { id: "a" } },
      { selector: { methods: ["POST", "GET"], path_template: "/search" }, policy: { id: "b" } }
    ]
  };
  assert.ok(validateRouteRateLimitTable(table).some((issue) => issue.code === "duplicate-route-selector"));
});

test("operation-id mismatch falls back instead of consuming the wrong route policy", () => {
  const table = {
    default_policy: { id: "default" },
    routes: [{
      selector: { methods: ["POST"], path_template: "/jobs/{job_id}", operation_id: "jobs.retry" },
      policy: { id: "retry" }
    }]
  };
  assert.equal(
    resolveRouteRateLimitPolicy(table, {
      method: "POST",
      path: "/jobs/123",
      route_template: "/jobs/{job_id}",
      operation_id: "jobs.cancel"
    })?.policy.id,
    "default"
  );
});
