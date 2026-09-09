import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

function exactKeys(value, keys, label) {
  assert(value !== null && typeof value === 'object' && !Array.isArray(value), label);
  assert.deepEqual(Object.keys(value).sort(), [...keys].sort(), `${label}: unknown or missing fields`);
}
function assertJson(value, depth = 0) {
  assert(depth <= 32, 'corpus nesting exceeds 32');
  if (typeof value === 'number') assert(Number.isFinite(value), 'non-finite corpus number');
  else if (Array.isArray(value)) for (const item of value) assertJson(item, depth + 1);
  else if (value !== null && typeof value === 'object') {
    assert(Object.getPrototypeOf(value) === Object.prototype || Object.getPrototypeOf(value) === null, 'non-JSON corpus object');
    for (const item of Object.values(value)) assertJson(item, depth + 1);
  } else assert(value === null || typeof value === 'string' || typeof value === 'boolean', 'non-JSON corpus value');
}

/** Corpus expectations are a third authored input, never inferred from either schema. */
export function assertCorpus(corpus) {
  exactKeys(corpus, ['schema', 'cases'], 'corpus');
  assert.equal(corpus.schema, 'ores.middleware.tjsv-corpus/v1');
  assert(Array.isArray(corpus.cases) && corpus.cases.length > 0 && corpus.cases.length <= 500, 'invalid corpus size');
  const ids = new Set();
  const expectations = new Map();
  for (const item of corpus.cases) {
    exactKeys(item, ['id', 'declaration', 'valid', 'instance'], 'corpus case');
    assert.match(item.id, /^[a-z][a-z0-9-]{0,79}$/u, 'unsafe case id');
    assert.match(item.declaration, /^[A-Z][A-Za-z0-9]{0,79}$/u, 'unsafe declaration');
    assert.equal(typeof item.valid, 'boolean', 'expectation must be explicit');
    assert(!ids.has(item.id), 'duplicate case id');
    ids.add(item.id);
    assertJson(item.instance);
    const outcomes = expectations.get(item.declaration) ?? new Set();
    outcomes.add(item.valid);
    expectations.set(item.declaration, outcomes);
  }
  for (const [name, outcomes] of expectations) assert.equal(outcomes.size, 2, `${name} requires positive and negative cases`);
  return corpus;
}

/** Materialize one fresh TJSV corpus from a reviewed manifest without changing expectations. */
export async function prepareCorpus(manifest, destination) {
  const bytes = await readFile(manifest);
  assert(bytes.length <= 1024 * 1024, 'corpus manifest exceeds 1 MiB');
  const corpus = assertCorpus(JSON.parse(bytes.toString('utf8')));
  // No recursive creation of the final directory: stale/reused output is an error.
  await mkdir(destination);
  for (const item of corpus.cases) {
    const parent = join(destination, item.declaration, item.valid ? 'valid' : 'invalid');
    await mkdir(parent, { recursive: true });
    await writeFile(join(parent, `${item.id}.json`), `${JSON.stringify(item.instance)}\n`, { flag: 'wx' });
  }
  return { corpus, digest: createHash('sha256').update(bytes).digest('hex') };
}

/** Every named declaration must have both kinds of fixture, and every fixture must execute. */
export function assertCorpusReport(corpus, report) {
  assertCorpus(corpus);
  const names = [...new Set(corpus.cases.map(item => item.declaration))].sort();
  assert(Array.isArray(report.declarationMap), 'missing report declaration map');
  const mapped = report.declarationMap.map(item => item.authored).sort();
  assert.deepEqual(names, mapped, 'corpus must cover every authored declaration exactly');
  assert.equal(report.differential?.summary?.corpusInstances, corpus.cases.length, 'not every corpus case was evaluated');
  assert.equal(report.configuration?.differential?.enabled, true, 'corpus validation disabled');
  assert.equal(report.coverage?.differentialInstanceValidation, true, 'corpus evidence missing');
}
