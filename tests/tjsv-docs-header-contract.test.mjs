import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';

test('header-map peer reconciliation stays flat and string-valued', () => {
  const schema = JSON.parse(readFileSync(new URL('../contracts/docs-serving.schema.json', import.meta.url), 'utf8'));
  // Equivalence with the original additionalProperties:string form is valid
  // here only because there are no named/pattern properties or applicators.
  // Genuine TypeSpec/JSON Schema parity remains the upstream TJSV job's duty.
  assert.deepEqual(schema.$defs.DocsDecision.properties.headers, {
    type: 'object', properties: {}, unevaluatedProperties: { type: 'string' },
  });
});
