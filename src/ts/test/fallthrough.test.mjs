import assert from "node:assert/strict";
import { test } from "node:test";

import {
  createFinalFallthroughResponse,
  nodeFinalFallthroughHandler,
  unmatchedRouteErrorCode
} from "../dist/fallthrough.js";

test("Fetch fallthrough defaults to 421 with a stable problem code", async () => {
  const response = createFinalFallthroughResponse(new Request("https://example.test/private/secret?x=1"));
  assert.equal(response.status, 421);
  assert.equal(response.headers.get("cache-control"), "no-store");
  assert.equal(response.headers.get("content-type"), "application/problem+json; charset=utf-8");
  const text = await response.text();
  const problem = JSON.parse(text);
  assert.equal(problem.code, unmatchedRouteErrorCode);
  assert.equal(problem.status, 421);
  assert.equal(text.includes("/private/secret"), false);
  assert.equal(text.includes("x=1"), false);
});

test("Fetch fallthrough supports explicit 404 compatibility", async () => {
  const response = createFinalFallthroughResponse(
    new Request("https://example.test/no-route"),
    { status: 404 }
  );
  assert.equal(response.status, 404);
  const problem = await response.json();
  assert.equal(problem.status, 404);
  assert.equal(problem.code, unmatchedRouteErrorCode);
});

test("HEAD omits the body while preserving the would-be content length", async () => {
  const getResponse = createFinalFallthroughResponse(new Request("https://example.test/no-route"));
  const getBody = await getResponse.text();
  const headResponse = createFinalFallthroughResponse(
    new Request("https://example.test/no-route", { method: "HEAD" })
  );
  assert.equal(await headResponse.text(), "");
  assert.equal(
    Number(headResponse.headers.get("content-length")),
    new TextEncoder().encode(getBody).byteLength
  );
});

test("native Node handler is final-handler compatible", () => {
  const headers = new Map();
  const response = {
    statusCode: 0,
    endedWith: Symbol("unset"),
    setHeader(name, value) {
      headers.set(name.toLowerCase(), String(value));
    },
    end(value) {
      this.endedWith = value;
    }
  };
  nodeFinalFallthroughHandler()({ method: "HEAD" }, response);
  assert.equal(response.statusCode, 421);
  assert.equal(response.endedWith, undefined);
  assert.equal(headers.get("cache-control"), "no-store");
  assert.ok(Number(headers.get("content-length")) > 0);
});
