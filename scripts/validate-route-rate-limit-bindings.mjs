#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import Ajv2020 from "ajv/dist/2020.js";

import {
  RouteRateLimitBindingResolutionError,
  resolveRouteRateLimitBinding,
  validateRouteRateLimitBindingTable
} from "../src/ts/dist/rate-limit-bindings.js";

const root = new URL("../", import.meta.url);
const schema = JSON.parse(await readFile(new URL("contracts/route-rate-limit/json-schema/authored.schema.json", root), "utf8"));
const corpus = JSON.parse(await readFile(new URL("contracts/route-rate-limit/fixtures/conformance.json", root), "utf8"));

assert.equal(corpus.schema, "ores.middleware.route-rate-limit-conformance/v1");
assert.equal(corpus.cases.length, 15, "the reviewed tranche must retain exactly 15 conformance cases");
assert.equal(new Set(corpus.cases.map((entry) => entry.id)).size, corpus.cases.length, "case ids must be unique");

const ajv = new Ajv2020({ allErrors: true, strict: true });
const validateSchema = ajv.compile(schema);

for (const entry of corpus.cases) {
  const schemaValid = validateSchema(entry.config);
  assert.equal(
    schemaValid,
    entry.schema_valid,
    `${entry.id}: authored JSON Schema verdict drift: ${JSON.stringify(validateSchema.errors ?? [])}`
  );

  const runtimeIssues = validateRouteRateLimitBindingTable(entry.config);
  assert.equal(
    runtimeIssues.length === 0,
    entry.runtime_valid,
    `${entry.id}: runtime validation drift: ${JSON.stringify(runtimeIssues)}`
  );

  if (!entry.runtime_valid || entry.request === undefined || entry.expect === undefined) continue;

  if (entry.expect.kind === "ambiguous") {
    assert.throws(
      () => resolveRouteRateLimitBinding(entry.config, entry.request),
      (error) => {
        assert(error instanceof RouteRateLimitBindingResolutionError, `${entry.id}: wrong error type`);
        assert.deepEqual(error.route_class_ids, entry.expect.route_class_ids, `${entry.id}: route classes drift`);
        assert.deepEqual(error.policy_ids, entry.expect.policy_ids, `${entry.id}: policy ids drift`);
        return true;
      }
    );
    continue;
  }

  const resolved = resolveRouteRateLimitBinding(entry.config, entry.request);
  if (entry.expect.kind === "none") {
    assert.equal(resolved, undefined, `${entry.id}: expected no binding`);
    continue;
  }

  assert(resolved !== undefined, `${entry.id}: expected binding`);
  assert.equal(resolved.source, entry.expect.kind, `${entry.id}: source drift`);
  assert.equal(resolved.policy_id, entry.expect.policy_id, `${entry.id}: policy id drift`);
  if (entry.expect.kind === "route") {
    assert.equal(resolved.route_class_id, entry.expect.route_class_id, `${entry.id}: route class drift`);
  } else {
    assert.equal(resolved.route_class_id, undefined, `${entry.id}: default must not invent route class`);
  }
}

console.log(`route-rate-limit-bindings: ${corpus.cases.length} shared schema/runtime cases passed`);
