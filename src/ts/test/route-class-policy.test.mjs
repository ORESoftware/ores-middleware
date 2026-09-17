import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  RouteClassPolicyResolutionError,
  resolveRouteClassPolicy,
  validateRouteClassPolicyTable
} from "../dist/route-class-policy.js";

const corpusUrl = new URL(
  "../../../contracts/route-class-policy/fixtures/conformance.json",
  import.meta.url
);
const corpus = JSON.parse(await readFile(corpusUrl, "utf8"));

test("shared route class policy corpus", () => {
  assert.equal(corpus.cases.length, 15, "reviewed corpus size drift");

  for (const entry of corpus.cases) {
    const violations = validateRouteClassPolicyTable(entry.table);
    if (!entry.expect.valid) {
      assert.ok(violations.length > 0, `${entry.id}: expected validation failure`);
      for (const code of entry.expect.codes) {
        assert.ok(
          violations.some((issue) => issue.code === code),
          `${entry.id}: missing ${code}; got ${JSON.stringify(violations)}`
        );
      }
      continue;
    }

    assert.deepEqual(violations, [], `${entry.id}: unexpected validation issues`);

    if (entry.expect.resolution_error === "unknown-route-class") {
      assert.throws(
        () => resolveRouteClassPolicy(entry.table, entry.route_class_id ?? undefined),
        (error) =>
          error instanceof RouteClassPolicyResolutionError
          && error.kind === "unknown-route-class",
        `${entry.id}: expected unknown route class failure`
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
});
