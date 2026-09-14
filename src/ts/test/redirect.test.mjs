import assert from "node:assert/strict";
import { test } from "node:test";

import {
  RedirectPolicyError,
  createRedirectResponse,
  resolveRedirect
} from "../dist/redirect.js";

const request = new Request("https://app.example.test/account?from=home");

test("relative redirect stays on the current origin", () => {
  const redirect = resolveRedirect(request, "/login?next=%2Faccount", { defaultStatus: 303 });
  assert.equal(redirect.location, "https://app.example.test/login?next=%2Faccount");
  assert.equal(redirect.status, 303);
});

test("cross-origin redirect requires explicit bare-origin allowlist", () => {
  assert.throws(
    () => resolveRedirect(request, "https://auth.example.test/login"),
    (error) => error instanceof RedirectPolicyError && error.code === "redirect_origin_forbidden"
  );
  const redirect = resolveRedirect(request, "https://auth.example.test/login", {
    allowExternalOrigins: ["https://auth.example.test"]
  });
  assert.equal(redirect.location, "https://auth.example.test/login");
});

test("protocol-relative, credential-bearing, and active-content redirects fail closed", () => {
  for (const target of [
    "//evil.example/path",
    "https://user:pass@app.example.test/path",
    "javascript:alert(1)",
    "data:text/html,hello"
  ]) {
    assert.throws(() => resolveRedirect(request, target), RedirectPolicyError);
  }
});

test("redirect response is no-store and has no body", async () => {
  const response = createRedirectResponse(request, "/signed-out", { defaultStatus: 307 });
  assert.equal(response.status, 307);
  assert.equal(response.headers.get("location"), "https://app.example.test/signed-out");
  assert.equal(response.headers.get("cache-control"), "no-store");
  assert.equal(await response.text(), "");
});
