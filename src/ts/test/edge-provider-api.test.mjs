import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  EdgeMinimalContractError,
  invokeEdgeMinimal
} from "../dist/edge-provider-api.js";
import middleware from "./fixtures/edge-minimal-no-imports.mjs";

const context = {
  requestId: "req-1",
  traceId: "trace-1",
  startedAtUnixMs: 1,
  baggage: {}
};

function dependencies() {
  const events = [];
  let fetchCalls = 0;
  const cache = new Map();

  return {
    events,
    get fetchCalls() {
      return fetchCalls;
    },
    providers: {
      fetch: async () => {
        fetchCalls += 1;
        return new Response("profile-v1", { status: 200 });
      },
      auth: {
        verify: async () => ({ userId: "user-123", claims: {} })
      },
      rateLimit: {
        allow: async () => true
      },
      cache: {
        get: async (key) => cache.get(key),
        set: async (key, value) => {
          cache.set(key, value);
        },
        delete: async (key) => {
          cache.delete(key);
        }
      },
      telemetry: {
        event: async (name, fields = {}) => {
          events.push({ name, fields });
        }
      }
    }
  };
}

test("authored edge_minimal middleware imports nothing and receives approved deps", async () => {
  const source = await readFile(
    new URL("./fixtures/edge-minimal-no-imports.mjs", import.meta.url),
    "utf8"
  );
  assert.doesNotMatch(source, /^\s*import\s/m);

  const deps = dependencies();
  const original = new Request("https://service.example.test/private", {
    headers: { "x-client": "mobile" }
  });

  const result = await invokeEdgeMinimal(middleware, original, context, deps.providers);
  assert.equal(result.kind, "continue");
  assert.equal(result.request.method, "GET");
  assert.equal(result.request.url, original.url);
  assert.equal(result.request.headers.get("x-ores-user-id"), "user-123");
  assert.equal(original.headers.get("x-ores-user-id"), null);
  assert.equal(deps.fetchCalls, 1);
  assert.equal(deps.events.length, 1);
  assert.equal(deps.events[0].name, "edge_minimal.authorized");
});

test("edge_minimal keeps authority and framing headers host-owned", async () => {
  const deps = dependencies();
  const callback = async ({ request }) => {
    request.setHeader("host", "attacker.example");
    return { kind: "continue" };
  };

  await assert.rejects(
    () => invokeEdgeMinimal(callback, new Request("https://service.example.test/"), context, deps.providers),
    (error) => error instanceof EdgeMinimalContractError && error.code === "edge_ingress_header_owned_by_host"
  );
});
