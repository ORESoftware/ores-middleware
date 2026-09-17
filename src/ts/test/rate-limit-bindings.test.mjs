import assert from "node:assert/strict";
import test from "node:test";

import {
  MAX_ROUTE_METHODS,
  MAX_ROUTE_RATE_LIMIT_BINDINGS,
  ROUTE_RATE_LIMIT_BINDING_SCHEMA,
  RouteRateLimitBindingResolutionError,
  resolveRouteRateLimitBinding,
  validateRouteRateLimitBindingTable
} from "../dist/rate-limit-bindings.js";

function binding(route_class_id, policy_id, methods, path_template, operation_id) {
  return {
    route_class_id,
    policy_id,
    selector: {
      methods,
      ...(path_template === undefined ? {} : { path_template }),
      ...(operation_id === undefined ? {} : { operation_id })
    }
  };
}

function table(routes = [], default_policy_id) {
  return {
    schema: ROUTE_RATE_LIMIT_BINDING_SCHEMA,
    ...(default_policy_id === undefined ? {} : { default_policy_id }),
    routes
  };
}

test("resolves a policy id without embedding numeric policy", () => {
  const config = table([
    binding("auth-login", "auth:login-strict", ["POST"], "/auth/login", "auth.login")
  ], "public-read-default");
  assert.deepEqual(validateRouteRateLimitBindingTable(config), []);
  assert.deepEqual(
    resolveRouteRateLimitBinding(config, {
      method: "POST",
      path: "/auth/login",
      route_template: "/auth/login",
      operation_id: "auth.login"
    }),
    { route_class_id: "auth-login", policy_id: "auth:login-strict", source: "route" }
  );
});

test("operation id beats a less specific parameterized path", () => {
  const config = table([
    binding("job-path", "jobs:default", ["POST"], "/jobs/{job_id}"),
    binding("job-retry", "jobs:retry", ["POST"], undefined, "jobs.retry")
  ]);
  assert.equal(
    resolveRouteRateLimitBinding(config, {
      method: "POST",
      path: "/jobs/42",
      route_template: "/jobs/{job_id}",
      operation_id: "jobs.retry"
    }).policy_id,
    "jobs:retry"
  );
});

test("parameterized concrete path ignores query string", () => {
  const config = table([
    binding("ledger-entry", "ledger:read", ["GET"], "/ledger/{ledger_id}/entries/:entry_id")
  ]);
  assert.deepEqual(validateRouteRateLimitBindingTable(config), []);
  assert.equal(
    resolveRouteRateLimitBinding(config, {
      method: "GET",
      path: "/ledger/a/entries/42?expand=true"
    }).policy_id,
    "ledger:read"
  );
});

test("default policy is explicit and no default stays unbound", () => {
  assert.deepEqual(
    resolveRouteRateLimitBinding(table([], "public:default"), { method: "GET", path: "/none" }),
    { policy_id: "public:default", source: "default" }
  );
  assert.equal(resolveRouteRateLimitBinding(table(), { method: "GET", path: "/none" }), undefined);
});

test("equal specificity fails closed with deterministic evidence", () => {
  const config = table([
    binding("users-b", "users:beta", ["GET"], "/users/:id"),
    binding("users-a", "users:alpha", ["GET"], "/users/{id}")
  ]);
  assert.throws(
    () => resolveRouteRateLimitBinding(config, { method: "GET", path: "/users/42" }),
    (error) => {
      assert(error instanceof RouteRateLimitBindingResolutionError);
      assert.deepEqual(error.route_class_ids, ["users-a", "users-b"]);
      assert.deepEqual(error.policy_ids, ["users:alpha", "users:beta"]);
      return true;
    }
  );
});

test("duplicate route class and selector are both rejected", () => {
  const route = binding("search", "search:read", ["GET"], "/search");
  const issues = validateRouteRateLimitBindingTable(table([
    route,
    { ...route, policy_id: "search:other" }
  ]));
  assert(issues.some((issue) => issue.code === "duplicate-route-class-id"));
  assert(issues.some((issue) => issue.code === "duplicate-route-selector"));
});

test("identifiers and method tokens fail closed", () => {
  const issues = validateRouteRateLimitBindingTable({
    schema: ROUTE_RATE_LIMIT_BINDING_SCHEMA,
    default_policy_id: "Bad Policy",
    routes: [binding("BadRoute", "also bad", ["-"], "/ok")]
  });
  assert(issues.some((issue) => issue.code === "invalid-route-class-id"));
  assert(issues.filter((issue) => issue.code === "invalid-policy-id").length >= 2);
  assert(issues.some((issue) => issue.code === "invalid-http-method"));
});

test("path parameter grammar and dot segments are rejected", () => {
  for (const path_template of ["/users/{bad-name}", "/users/{id", "/users/..", "/a/*/b"]) {
    const issues = validateRouteRateLimitBindingTable(
      table([binding("bad-route", "bad:route", ["GET"], path_template)])
    );
    assert(issues.some((issue) => issue.path.endsWith("selector.path_template")), path_template);
  }
});

test("table and method list counts are bounded", () => {
  const routes = Array.from({ length: MAX_ROUTE_RATE_LIMIT_BINDINGS + 1 }, (_, index) =>
    binding(`route-${index}`, `route:${index}`, ["GET"], `/route/${index}`)
  );
  assert(
    validateRouteRateLimitBindingTable(table(routes))
      .some((issue) => issue.code === "too-many-route-bindings")
  );

  const methods = Array.from({ length: MAX_ROUTE_METHODS + 1 }, (_, index) => `X${index}`);
  assert(
    validateRouteRateLimitBindingTable(table([
      binding("method-heavy", "method:heavy", methods, "/method-heavy")
    ])).some((issue) => issue.code === "too-many-http-methods")
  );
});

test("schema identifier is versioned and exact", () => {
  assert(
    validateRouteRateLimitBindingTable({ schema: "route-bindings/v0", routes: [] })
      .some((issue) => issue.code === "invalid-binding-schema")
  );
});
