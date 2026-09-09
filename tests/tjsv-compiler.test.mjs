import assert from 'node:assert/strict';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { loadPinnedValidator, checkOptions } from '../scripts/tjsv-check.mjs';
import { assertPassingReport } from '../scripts/tjsv-evidence.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
// Literal, independently written fixtures. Neither authority is emitted from the other.
const typespec = 'model Envelope { payload: string; }\n';
const schema = {
  $schema: 'https://json-schema.org/draft/2020-12/schema',
  $id: 'https://schemas.oresoftware.com/middleware/tjsv-control.json',
  $defs: { Envelope: { type: 'object', properties: { payload: { type: 'string' } },
    required: ['payload'], additionalProperties: false } },
};

async function fixture() {
  const { validator, toolRoot } = await loadPinnedValidator(root);
  const parent = join(root, 'target/tjsv/controls');
  await mkdir(parent, { recursive: true });
  const directory = await mkdtemp(join(parent, 'case-'));
  const source = join(directory, 'authored');
  await mkdir(source);
  const tsp = join(source, 'main.tsp');
  const json = join(source, 'schema.json');
  await writeFile(tsp, typespec, { flag: 'wx' });
  await writeFile(json, `${JSON.stringify(schema, null, 2)}\n`, { flag: 'wx' });
  async function check(name, instances) {
    const before = await Promise.all([readFile(tsp), readFile(json)]);
    const report = await validator.runCheck(checkOptions(tsp, json, join(directory, name, 'witness'), toolRoot, instances));
    await validator.writeReport(join(directory, `${name}.json`), report);
    assert.deepEqual(await readFile(tsp), before[0], 'compiler must not edit TypeSpec');
    assert.deepEqual(await readFile(json), before[1], 'compiler must not edit authored JSON Schema');
    return report;
  }
  return { directory, tsp, json, check };
}

test('real pinned TJSV accepts independently authored matching contracts', async () => {
  const f = await fixture();
  assertPassingReport(await f.check('positive'));
});

test('real pinned TJSV rejects scalar drift with an instance-verdict counterexample', async () => {
  const f = await fixture();
  const changed = structuredClone(schema);
  changed.$defs.Envelope.properties.payload.type = 'number';
  await writeFile(f.json, JSON.stringify(changed));
  const report = await f.check('scalar-drift');
  assert.equal(report.status, 'stopped_for_evaluation');
  assert(report.counts.structuralFindings > 0);
  assert(report.differential.summary.divergences > 0);
  assert(report.findings.some(item => item.ruleId === 'instance-verdict-divergence'));
  assert.throws(() => assertPassingReport(report));
});

test('agreement cannot override independently authored corpus expectations', async () => {
  const f = await fixture();
  const corpus = join(f.directory, 'instances');
  await mkdir(join(corpus, 'Envelope/valid'), { recursive: true });
  await writeFile(join(corpus, 'Envelope/valid/number.json'), '{"payload":42}\n');
  const report = await f.check('corpus-expectation', corpus);
  assert.equal(report.status, 'stopped_for_evaluation');
  assert.equal(report.differential.summary.divergences, 0);
  assert.equal(report.differential.summary.corpusInstances, 1);
  assert(report.findings.some(item => item.ruleId === 'corpus-instance-rejected'));
  assert.throws(() => assertPassingReport(report));
});
