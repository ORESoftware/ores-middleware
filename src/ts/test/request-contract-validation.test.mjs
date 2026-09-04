import assert from "node:assert/strict";
import test from "node:test";

import {
  checkRequestContract,
  createMiddleware,
  currentContext,
  defaultConfig
} from "../dist/index.js";

function config() {
  const value = defaultConfig("request-contract-test");
  value.environment = "test";
  value.settings.rateLimit.enabled = false;
  value.settings.idempotency.enabled = false;
  value.settings.compression.enabled = false;
  value.settings.securityHeaders.enabled = false;
  return value;
}

function jsonRequest(url, body, headers = {}) {
  return new Request(url, {
    method: "POST",
    headers: { "content-type": "application/json", ...headers },
    body: JSON.stringify(body)
  });
}

test("resolver identity is method + pathname only and validator receives immutable surfaces", async () => {
  const resolved = [];
  const validated = [];
  const validator = {
    resolve(method, pathname, ...unexpected) {
      resolved.push([method, pathname, unexpected.length]);
      return {
        pathTemplate: "/v1/items/{id}",
        pathParams: { id: "42" },
        async validate(input) {
          assert.equal(Object.isFrozen(input), true);
          assert.equal(Object.isFrozen(input.pathParams), true);
          assert.equal(Object.isFrozen(input.query), true);
          assert.equal(Object.isFrozen(input.headers), true);
          assert.equal(Object.isFrozen(input.query[0]), true);
          assert.equal(Object.isFrozen(input.headers[0]), true);
          validated.push({
            method: input.method,
            pathname: input.pathname,
            pathTemplate: input.pathTemplate,
            pathParams: input.pathParams,
            query: input.query,
            headers: input.headers,
            body: await input.request.json()
          });
          return [];
        }
      };
    }
  };

  const middleware = createMiddleware(config(), { requestContractValidator: validator });
  const inputs = [
    jsonRequest("https://example.test/v1/items/42?view=full", { name: "alpha" }, { "x-client-version": "1" }),
    jsonRequest("https://example.test/v1/items/42?view=compact", { name: "beta" }, { "x-client-version": "2" })
  ];

  for (const request of inputs) {
    const response = await middleware(request, async (handlerRequest) => {
      const body = await handlerRequest.json();
      return Response.json({ body });
    });
    assert.equal(response.status, 200);
  }

  assert.deepEqual(resolved, [
    ["POST", "/v1/items/42", 0],
    ["POST", "/v1/items/42", 0]
  ]);
  assert.equal(validated[0].query[0][1], "full");
  assert.equal(validated[1].query[0][1], "compact");
  assert.equal(validated[0].headers.some(([name, value]) => name === "x-client-version" && value === "1"), true);
  assert.deepEqual(validated.map((entry) => entry.body.name), ["alpha", "beta"]);
  assert.equal(currentContext(), undefined);
});

test("invalid path/query/header/body surfaces fail before rate limiting, auth, and handler execution", async () => {
  let rateCalls = 0;
  let authCalls = 0;
  let handlerCalls = 0;
  const value = config();
  value.settings.rateLimit.enabled = true;

  const middleware = createMiddleware(value, {
    requestContractValidator: {
      resolve() {
        return {
          pathTemplate: "/v1/items/{id}",
          pathParams: { id: "not-a-uuid" },
          validate() {
            return [{ path: "/headers/x-client-version", code: "required", message: "header is required" }];
          }
        };
      }
    },
    rateLimiter: {
      async allow() {
        rateCalls += 1;
        return true;
      }
    },
    async authVerifier() {
      authCalls += 1;
      return { userId: "user-1" };
    }
  });

  const response = await middleware(
    jsonRequest("https://example.test/v1/items/not-a-uuid", { name: 7 }),
    async () => {
      handlerCalls += 1;
      return Response.json({ ok: true });
    }
  );

  assert.equal(response.status, 400);
  const body = await response.json();
  assert.equal(body.title, "request_contract_validation_failed");
  assert.equal(rateCalls, 0);
  assert.equal(authCalls, 0);
  assert.equal(handlerCalls, 0);
  assert.equal(currentContext(), undefined);
});

test("strict validator returns 404 when method + pathname have no declared operation", async () => {
  const middleware = createMiddleware(config(), {
    requestContractValidator: { resolve: () => undefined }
  });
  const response = await middleware(
    new Request("https://example.test/missing"),
    async () => Response.json({ unreachable: true })
  );
  assert.equal(response.status, 404);
  assert.equal((await response.json()).title, "unknown_operation");
});

test("validator issue arrays are runtime checked and malformed adapters fail closed", async () => {
  const request = new Request("https://example.test/v1/items/42");
  await assert.rejects(
    () => checkRequestContract(
      {
        resolve: () => ({
          pathTemplate: "/v1/items/{id}",
          validate: () => [{ path: 42, code: "bad", message: "bad" }]
        })
      },
      request
    ),
    /issue 0 is malformed/
  );
});
