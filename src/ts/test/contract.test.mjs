import test from "node:test";
import assert from "node:assert/strict";

import { capabilities, currentContext, defaultConfig, descriptor, runWithContext, validateConfig } from "../dist/index.js";

test("descriptor exports the standard semantic operations", () => {
  const value = descriptor();
  assert.equal(Object.keys(value.operationSymbols).length, 7);
  assert.equal(value.capabilities.length, capabilities.length);
});

test("production rejects test-only middleware", () => {
  const config = defaultConfig("test");
  config.environment = "production";
  config.settings.faultInjection.enabled = true;
  config.settings.testAuthBypass.enabled = true;
  const issues = validateConfig(config);
  assert.ok(issues.some((issue) => issue.path.includes("faultInjection")));
  assert.ok(issues.some((issue) => issue.path.includes("testAuthBypass")));
});

test("async local context is scoped", async () => {
  await runWithContext({ requestId: "r1", traceId: "0123456789abcdef0123456789abcdef", startedAtUnixMs: 0, baggage: {} }, async () => {
    await Promise.resolve();
    assert.equal(currentContext()?.requestId, "r1");
  });
  assert.equal(currentContext(), undefined);
});
