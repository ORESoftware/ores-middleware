import assert from 'node:assert/strict';
import test from 'node:test';
import { applyLanePolicy, checkOptions } from '../scripts/tjsv-check.mjs';

const lane = { id: 'persistence', typespec: 'contracts/persistence/idempotency-record.tsp',
  authoredSchema: 'contracts/persistence/idempotency-record.schema.json' };
const source = () => ({ $defs: { IdempotencyRecord: { type: 'object', additionalProperties: false } } });
const options = () => checkOptions('/contracts/source.tsp', '/contracts/source.json', '/target/witness', '/tool');

test('explicit persistence closure preserves every other strict check option', () => {
  const before = options();
  const after = applyLanePolicy(lane, before, source());
  assert.deepEqual(after, { ...before, sealObjectSchemas: false });
  assert.equal(before.sealObjectSchemas, true, 'options must not be mutated');
});
for (const id of ['middleware', 'docs-serving', 'function-bodies', 'rate-limit-v2']) {
  test(`${id} keeps generic emitter sealing`, () => {
    const before = options();
    assert.equal(applyLanePolicy({ id }, before, undefined), before);
    assert.equal(before.sealObjectSchemas, true);
  });
}
for (const [key, value] of [['typespec', 'contracts/other.tsp'], ['authoredSchema', 'contracts/other.json']]) {
  test(`explicit policy rejects a changed ${key} identity`, () => {
    assert.throws(() => applyLanePolicy({ ...lane, [key]: value }, options(), source()));
  });
}
for (const value of [undefined, true, {}, 'false']) {
  test(`explicit policy rejects unclosed model ${JSON.stringify(value)}`, () => {
    const schema = source();
    schema.$defs.IdempotencyRecord.additionalProperties = value;
    assert.throws(() => applyLanePolicy(lane, options(), schema));
  });
}
test('explicit policy rejects an absent or non-object model', () => {
  assert.throws(() => applyLanePolicy(lane, options(), {}));
  const schema = source(); schema.$defs.IdempotencyRecord.type = 'string';
  assert.throws(() => applyLanePolicy(lane, options(), schema));
});
for (const key of ['$ref', '$dynamicRef', 'allOf', 'anyOf', 'oneOf', 'not', 'if',
  'then', 'else', 'dependentSchemas', 'patternProperties', 'unevaluatedProperties']) {
  test(`explicit policy refuses unreviewed ${key} composition`, () => {
    const schema = source(); schema.$defs.IdempotencyRecord[key] = {};
    assert.throws(() => applyLanePolicy(lane, options(), schema), /requires review/);
  });
}
