#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import Ajv2020 from "ajv/dist/2020.js";

import {
  RouteClassPolicyResolutionError,
  resolveRouteClassPolicy,
  validateRouteClassPolicyTable
} from "../src/ts/dist/route-class-policy.js";

const root = new URL("../", import.meta.url);
const schema = JSON.parse(
  await readFile(
    new URL("contracts/route-class-policy/json-schema/authored.schema.json", root),
    "utf8"
  )
);
const corpus = JSON.parse(
  await readFile(
    new URL("contracts/route-class-policy/fixtures/conformance.json", root),
    "utf8"
  )
);

assert.equal(corpus.schema, "ores.middleware.route-class-policy-conformance/v1");
assert.equal(corpus.cases.length, 15, "route class policy tranche must retain exactly 15 cases");
assert.equal(
  new Set(corpus.cases.map((entry) => entry.id)).size,
  corpus.cases.length,
  "route class policy case ids must be unique"
);

const ajv = new Ajv2020({ allErrors: true, strict: true });
ajv.addSchema(schema, schema.$id);
const validateSchema = ajv.compile({ $ref: schema.$id });

for (const entry of corpus.cases) {
  const schemaValid = validateSchema(entry.table);
  assert.equal(
    schemaValid,
    entry.schema_valid,
    `${entry.id}: authored JSON Schema verdict drift: ${JSON.stringify(validateSchema.errors ?? [])}`
  );

  const violations = validateRouteClassPolicyTable(entry.table);
  assert.equal(
    violations.length === 0,
    entry.expect.valid,
    `${entry.id}: runtime validation drift: ${JSON.stringify(violations)}`
  );

  if (!entry.expect.valid) {
    for (const code of entry.expect.codes) {
      assert.ok(
        violations.some((issue) => issue.code === code),
        `${entry.id}: missing ${code}; got ${JSON.stringify(violations)}`
      );
    }
    continue;
  }

  if (entry.expect.resolution_error === "unknown-route-class") {
    assert.throws(
      () => resolveRouteClassPolicy(entry.table, entry.route_class_id ?? undefined),
      (error) =>
        error instanceof RouteClassPolicyResolutionError
        && error.kind === "unknown-route-class",
      `${entry.id}: expected unknown route class resolution failure`
    );
    continue;
  }

  const resolved = resolveRouteClassPolicy(
    entry.table,
    entry.route_class_id ?? undefined
  );
  assert.equal(
    resolved.route_class_id,
    entry.expect.resolved.route_class_id,
    `${entry.id}: route class drift`
  );
  assert.deepEqual(
    resolved.lineage,
    entry.expect.resolved.lineage,
    `${entry.id}: lineage drift`
  );

  for (const [key, value] of Object.entries(entry.expect.resolved.policies)) {
    assert.equal(
      resolved.policies[key],
      value,
      `${entry.id}: policy ${key} drift`
    );
  }
}

console.log(`route-class-policy: ${corpus.cases.length} shared cases passed`);
