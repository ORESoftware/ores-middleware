import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { assertCorpus, prepareCorpus, assertCorpusReport } from '../scripts/tjsv-corpus.mjs';

const seed = () => ({ schema: 'ores.middleware.tjsv-corpus/v1', cases: [
  { id: 'valid', declaration: 'Envelope', valid: true, instance: { payload: 'ok' } },
  { id: 'invalid', declaration: 'Envelope', valid: false, instance: { payload: null } },
] });
const report = () => ({ declarationMap: [{ authored: 'Envelope' }],
  differential: { summary: { corpusInstances: 2 } }, configuration: { differential: { enabled: true } },
  coverage: { differentialInstanceValidation: true } });

test('valid independently authored positive and negative corpus', () => assertCorpus(seed()));
for (const [name, mutate] of Object.entries({
  'empty cases': c => { c.cases = []; },
  'missing schema': c => { delete c.schema; },
  'wrong schema': c => { c.schema = 'other/v1'; },
  'implicit expectation': c => { delete c.cases[0].valid; },
  'string expectation': c => { c.cases[0].valid = 'true'; },
  'unknown skip switch': c => { c.cases[0].skip = true; },
  'missing instance': c => { delete c.cases[0].instance; },
  'duplicate id': c => { c.cases[1].id = c.cases[0].id; },
  'path traversal id': c => { c.cases[0].id = '../escape'; },
  'path traversal declaration': c => { c.cases[0].declaration = '../Escape'; },
  'backslash id': c => { c.cases[0].id = 'some\\where'; },
  'nonfinite number': c => { c.cases[0].instance = Infinity; },
  'nested nonfinite number': c => { c.cases[0].instance = { a: [NaN] }; },
  'undefined': c => { c.cases[0].instance = undefined; },
  'date instead of JSON': c => { c.cases[0].instance = new Date(0); },
  'no negative case': c => { c.cases[1].valid = true; },
  'no positive case': c => { c.cases[0].valid = false; },
  'missing peer expectation': c => { c.cases[1].declaration = 'Other'; },
  'too many cases': c => { c.cases = Array(501).fill(c.cases[0]); },
  'excessive depth': c => { let x = {}; for (let i = 0; i < 40; i++) x = { x }; c.cases[0].instance = x; },
})) test(`rejects ${name}`, () => { const c = seed(); mutate(c); assert.throws(() => assertCorpus(c)); });

test('valid consumed corpus evidence', () => assertCorpusReport(seed(), report()));
for (const [name, mutate] of Object.entries({
  'missing declaration': r => { r.declarationMap = []; },
  'extra declaration': r => { r.declarationMap.push({ authored: 'Other' }); },
  'duplicate declaration': r => { r.declarationMap.push({ authored: 'Envelope' }); },
  'skipped case': r => { r.differential.summary.corpusInstances = 1; },
  'unexpected case': r => { r.differential.summary.corpusInstances = 3; },
  'disabled lane': r => { r.configuration.differential.enabled = false; },
  'missing coverage': r => { delete r.coverage.differentialInstanceValidation; },
})) test(`rejects corpus report with ${name}`, () => { const r = report(); mutate(r); assert.throws(() => assertCorpusReport(seed(), r)); });

test('materialization preserves exact values, independent expectations and manifest digest', async t => {
  const root = await mkdtemp(join(tmpdir(), 'tjsv-corpus-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const manifest = join(root, 'manifest.json');
  await writeFile(manifest, JSON.stringify(seed()));
  const out = join(root, 'instances');
  const { corpus, digest } = await prepareCorpus(manifest, out);
  assert.deepEqual(corpus, seed());
  assert.match(digest, /^[a-f0-9]{64}$/u);
  assert.deepEqual(JSON.parse(await readFile(join(out, 'Envelope/valid/valid.json'), 'utf8')), { payload: 'ok' });
  assert.deepEqual(JSON.parse(await readFile(join(out, 'Envelope/invalid/invalid.json'), 'utf8')), { payload: null });
  await assert.rejects(() => prepareCorpus(manifest, out), /EEXIST/u);
});

test('JSON own-property keys survive materialization without prototype interpretation', async t => {
  const root = await mkdtemp(join(tmpdir(), 'tjsv-corpus-own-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const c = seed();
  c.cases[0].instance = JSON.parse('{"__proto__":"literal","constructor":"literal"}');
  const path = join(root, 'manifest.json');
  await writeFile(path, JSON.stringify(c));
  await prepareCorpus(path, join(root, 'instances'));
  const actual = JSON.parse(await readFile(join(root, 'instances/Envelope/valid/valid.json'), 'utf8'));
  assert(Object.hasOwn(actual, '__proto__'));
  assert.equal(actual.__proto__, 'literal');
  assert.equal(actual.constructor, 'literal');
});

import { assertMatrix } from '../scripts/tjsv-evidence.mjs';
const matrix = JSON.parse(await readFile(new URL('../contracts/tjsv.matrix.json', import.meta.url), 'utf8'));
test('docs lane explicitly requires the reviewed corpus', () => {
  assertMatrix(matrix);
  assert.equal(matrix.lanes.find(lane => lane.id === 'docs-serving').corpus, 'contracts/docs-serving.cases.json');
});
for (const path of [null, '/tmp/cases.json', 'contracts/../cases.json', 'contracts/cases', 'contracts//cases.json', 'contracts/cases\\other.json', 'contracts/docs-serving.schema.json']) {
  test(`matrix refuses unsafe or non-independent corpus ${JSON.stringify(path)}`, () => {
    const changed = structuredClone(matrix);
    changed.lanes.find(lane => lane.id === 'docs-serving').corpus = path;
    assert.throws(() => assertMatrix(changed));
  });
}
