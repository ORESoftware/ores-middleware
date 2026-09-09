import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import Ajv2020 from 'ajv/dist/2020.js';
import { assertCorpus } from '../scripts/tjsv-corpus.mjs';

const root = new URL('../', import.meta.url);
const previousBytes = await readFile(new URL('tests/fixtures/docs-serving-v1.schema.json', root));
const previous = JSON.parse(previousBytes);
const current = JSON.parse(await readFile(new URL('contracts/docs-serving.schema.json', root), 'utf8'));
const corpus = assertCorpus(JSON.parse(await readFile(new URL('contracts/docs-serving.cases.json', root), 'utf8')));
const names = ['DocsAction', 'DocsDecision', 'DocsHeaders', 'DocsRepresentation', 'DocsRequest'];

function validators(schema, legacy) {
  const defs = { ...schema.$defs };
  if (legacy) defs.DocsHeaders = schema.$defs.DocsDecision.properties.headers;
  const ajv = new Ajv2020({ strict: true, allErrors: true, ownProperties: true });
  return new Map(names.map(name => [name, ajv.compile({
    $schema: 'https://json-schema.org/draft/2020-12/schema',
    $defs: defs,
    $ref: `#/$defs/${name}`,
  })]));
}
const before = validators(previous, true);
const after = validators(current, false);

test('legacy comparison evidence is exactly the existing v1 author, not a generated schema', () => {
  const blob = createHash('sha1').update(`blob ${previousBytes.length}\0`).update(previousBytes).digest('hex');
  assert.equal(blob, '45735e7d6890f31240311636c98be77e5183f69a');
});

test('all five declarations have independent positive and negative fixtures', () => {
  assert.deepEqual([...new Set(corpus.cases.map(item => item.declaration))].sort(), names);
  assert.equal(corpus.cases.length, 87, 'review corpus changes rather than silently dropping cases');
});

test('the reconciliation changes only named-map representation and simple-object closure spelling', () => {
  const expected = structuredClone(previous);
  for (const name of ['DocsRequest', 'DocsDecision']) {
    const node = expected.$defs[name];
    // This proof is deliberately not applied to inheritance/conditional/composed schemas.
    assert.deepEqual(Object.keys(node).sort(), ['additionalProperties', 'properties', 'required', 'type']);
    assert.equal(node.additionalProperties, false);
    delete node.additionalProperties;
    node.unevaluatedProperties = false;
  }
  expected.$defs.DocsHeaders = { type: 'object', properties: {}, unevaluatedProperties: { type: 'string' } };
  expected.$defs.DocsDecision.properties.headers = { $ref: '#/$defs/DocsHeaders' };
  assert.deepEqual(current, expected);
});

for (const item of corpus.cases) {
  test(`docs wire compatibility: ${item.id}`, () => {
    const oldAccepts = before.get(item.declaration)(item.instance);
    const newAccepts = after.get(item.declaration)(item.instance);
    assert.equal(oldAccepts, item.valid, 'independent fixture disagrees with original v1 contract');
    assert.equal(newAccepts, item.valid, 'reconciled contract changed accepted values');
  });
}

test('top-level request/decision union remains unchanged on every boundary fixture', () => {
  const oldRoot = new Ajv2020({ strict: true, ownProperties: true }).compile(previous);
  const newRoot = new Ajv2020({ strict: true, ownProperties: true }).compile(current);
  for (const item of corpus.cases.filter(item => ['DocsRequest', 'DocsDecision'].includes(item.declaration))) {
    assert.equal(oldRoot(item.instance), item.valid, `old root: ${item.id}`);
    assert.equal(newRoot(item.instance), item.valid, `new root: ${item.id}`);
  }
});
