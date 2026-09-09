import assert from 'node:assert/strict';
import { before, test } from 'node:test';
import { execFileSync } from 'node:child_process';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import Ajv2020 from 'ajv/dist/2020.js';
import addFormats from 'ajv-formats';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const TJSV_REVISION = '4473504c4c9d2831d825919f70c03994d8ce01d2';
const TOOL = join(ROOT, 'target/tools/tjsv');
const BASELINE = '02c017a21c6daa95f1d2dc2436b1104fad4275f9';
const INPUTS = [
  'contracts/persistence/idempotency-record.tsp',
  'contracts/persistence/idempotency-record.schema.json',
  'fixtures/generated-runtime-conformance.json',
  'tests/persistence-tjsv-salvage.test.mjs',
  '.github/workflows/persistence-tjsv-salvage.yml',
  'package.json', 'package-lock.json',
];
const git = (...args) => execFileSync('git', ['-C', ROOT, ...args], { encoding: 'utf8', timeout: 30_000 }).trim();
let validator, source, schema, baselineSchema, corpus, output, sourceCommit;

before(async () => {
  sourceCommit = git('rev-parse', 'HEAD');
  if (process.env.EXPECTED_SOURCE_SHA) assert.equal(sourceCommit, process.env.EXPECTED_SOURCE_SHA);
  assert.equal(git('status', '--porcelain', '--untracked-files=all', '--', ...INPUTS), '');
  assert.equal(execFileSync('git', ['-C', TOOL, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(), TJSV_REVISION);
  validator = await import(pathToFileURL(join(TOOL, 'src/index.mjs')).href);
  source = await readFile(join(ROOT, INPUTS[0]), 'utf8');
  schema = JSON.parse(await readFile(join(ROOT, INPUTS[1]), 'utf8'));
  baselineSchema = JSON.parse(git('show', `${BASELINE}:${INPUTS[1]}`));
  corpus = JSON.parse(await readFile(join(ROOT, INPUTS[2]), 'utf8'));
  assert.equal(corpus.authority, 'independent-fixture-corpus');
  assert.equal(corpus.model, 'IdempotencyRecord');
  assert(corpus.cases.length > 0);
  output = await mkdtemp(join(ROOT, 'target/persistence-tjsv-salvage-'));
});

async function check(name, { tsp = source, authored = schema, cases = corpus.cases } = {}) {
  const dir = join(output, name);
  await mkdir(dir, { recursive: true });
  const tspPath = join(dir, 'main.tsp');
  const schemaPath = join(dir, 'schema.json');
  await writeFile(tspPath, tsp, { flag: 'wx' });
  await writeFile(schemaPath, JSON.stringify(authored), { flag: 'wx' });
  const instances = join(dir, 'instances');
  for (const item of cases) {
    const lane = item.expect === 'accept' ? 'valid' : 'invalid';
    const location = join(instances, 'IdempotencyRecord', lane);
    await mkdir(location, { recursive: true });
    await writeFile(join(location, `${item.id}.json`), JSON.stringify(item.value), { flag: 'wx' });
  }
  const report = await validator.runCheck({
    typespec: tspPath,
    authoredSchema: schemaPath,
    instances,
    outputDir: join(dir, 'witness'),
    tspBin: join(TOOL, 'node_modules/.bin/tsp'),
    maxFindings: 1000,
    maxProbes: 128,
    probes: true,
    formatAssertion: true,
    int64Strategy: 'number',
    sealObjectSchemas: true,
  });
  await validator.writeReport(join(dir, 'report.json'), report);
  assert.equal(report.toolchain.typespecCompiler.available, true);
  assert.equal(report.counts.findingsTruncated, false);
  return report;
}

function assertPass(report) {
  assert.equal(report.status, 'passed', JSON.stringify(report.findings));
  assert.equal(report.zeroUnexplainedFindings, true);
  assert.equal(report.counts.findings, 0);
  assert.equal(report.counts.structuralFindings, 0);
  assert.equal(report.counts.differentialFindings, 0);
  assert.equal(report.counts.typespecDeclarations, 2);
  assert.equal(report.counts.authoredDeclarations, 2);
  assert.equal(report.counts.generatedDeclarations, 2);
  assert.equal(report.differential.summary.comparedDeclarations, 2);
  assert.equal(report.differential.summary.corpusInstances, corpus.cases.length);
  assert.equal(report.differential.summary.divergences, 0);
  assert.equal(report.differential.summary.refusals, 0);
}

function assertBlocked(report) {
  assert.equal(report.status, 'stopped_for_evaluation');
  assert.equal(report.zeroUnexplainedFindings, false);
  assert(report.counts.findings > 0);
}

function compileSchema(document) {
  const ajv = new Ajv2020({ allErrors: true, strict: false });
  addFormats(ajv);
  return ajv.compile(document);
}

test('current peer authorities and corpus pass actual pinned TJSV', async () => {
  assertPass(await check('positive'));
});

test('dual flat-object closure preserves reviewed pre-merge wire verdicts', () => {
  const oldValidate = compileSchema(baselineSchema);
  const newValidate = compileSchema(schema);
  for (const item of corpus.cases) {
    const expected = item.expect === 'accept';
    assert.equal(oldValidate(item.value), expected, `${item.id}: baseline`);
    assert.equal(newValidate(item.value), expected, `${item.id}: current`);
  }
  const model = schema.$defs.IdempotencyRecord;
  assert.equal(model.additionalProperties, false);
  assert.equal(model.unevaluatedProperties, false);
  for (const keyword of ['allOf', 'anyOf', 'oneOf', 'patternProperties', 'if', 'then', 'else']) {
    assert(!Object.hasOwn(model, keyword), `composition requires new closure review: ${keyword}`);
  }
  assert.equal(model['x-ores-sql'].table, 'middleware_idempotency');
  assert.deepEqual(model['x-ores-sql'].primaryKey, ['id']);
  assert.deepEqual(model['x-ores-sql'].unique, [['tenantId', 'idempotencyKey']]);
});

test('TJSV rejects one-sided SQL table metadata drift', async () => {
  const tsp = source.replace('table: "middleware_idempotency"', 'table: "wrong_table"');
  assert.notEqual(tsp, source);
  const report = await check('sql-table-drift', { tsp });
  assertBlocked(report);
  assert(report.counts.structuralFindings > 0);
});

test('TJSV rejects one-sided uniqueness metadata drift', async () => {
  const authored = structuredClone(schema);
  authored.$defs.IdempotencyRecord['x-ores-sql'].unique = [['idempotencyKey']];
  const report = await check('sql-unique-drift', { authored });
  assertBlocked(report);
  assert(report.counts.structuralFindings > 0);
});

test('TJSV rejects tenant scalar drift with real verdict divergences', async () => {
  const authored = structuredClone(schema);
  authored.$defs.IdempotencyRecord.properties.tenantId = { type: 'integer' };
  const report = await check('tenant-scalar-drift', { authored });
  assertBlocked(report);
  assert(report.differential.summary.divergences > 0);
});

test('TJSV rejects opening both closure guards to unknown properties', async () => {
  const authored = structuredClone(schema);
  delete authored.$defs.IdempotencyRecord.additionalProperties;
  delete authored.$defs.IdempotencyRecord.unevaluatedProperties;
  const report = await check('unknown-property-drift', { authored });
  assertBlocked(report);
  assert(report.differential.summary.divergences > 0);
});

test('independent corpus expectations can block agreeing validators', async () => {
  const cases = structuredClone(corpus.cases);
  const item = cases.find(({ id }) => id === 'valid-minimal');
  assert(item);
  item.expect = 'reject';
  const report = await check('contradictory-corpus', { cases });
  assertBlocked(report);
  assert.equal(report.counts.structuralFindings, 0);
  assert(report.counts.differentialFindings > 0);
});

test('source binding remains exact after all controls', () => {
  assert.equal(git('rev-parse', 'HEAD'), sourceCommit);
  assert.equal(git('status', '--porcelain', '--untracked-files=all', '--', ...INPUTS), '');
});
