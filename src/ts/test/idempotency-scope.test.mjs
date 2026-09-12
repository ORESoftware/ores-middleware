import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
import { scopedIdempotencyKey } from "../dist/idempotency-scope.js";
const corpus = JSON.parse(await readFile(new URL("../../../contracts/fixtures/idempotency-scope.json", import.meta.url), "utf8"));
for (const item of corpus.cases) {
  test(`shared replay-scope vector: ${item.name}`, async () => {
    assert.equal(await scopedIdempotencyKey(item.scope), item.expectedKey);
  });
}
test("all intended replay scopes are distinct", async () => {
  const keys = await Promise.all(corpus.cases.map(item => scopedIdempotencyKey(item.scope)));
  assert.equal(new Set(keys).size, keys.length);
});
test("malformed replay scopes fail closed rather than silently aliasing", async () => {
  const base = corpus.cases[0].scope;
  for (const scope of [{ ...base, userId: "\ud800" }, { ...base, path: "relative" }, { ...base, idempotencyKey: "" }, { ...base, tenantId: 1 }, { ...base, extra: true }]) {
    await assert.rejects(scopedIdempotencyKey(scope), TypeError);
  }
});
