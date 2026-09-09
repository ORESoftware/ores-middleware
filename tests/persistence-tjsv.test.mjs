import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import Ajv2020 from 'ajv/dist/2020.js';
import addFormats from 'ajv-formats';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const TJSV_REVISION = '4473504c4c9d2831d825919f70c03994d8ce01d2';
const TOOL = join(ROOT, 'target/tools/tjsv');
const INPUTS = [
  'contracts/persistence/idempotency-record.tsp',
  'contracts/persistence/idempotency-record.schema.json',
  'fixtures/generated-runtime-conformance.json',
  'tests/persistence-tjsv.test.mjs',
  '.github/workflows/persistence-tjsv.yml',
  'package.json', 'package-lock.json',
];
const git = (root, ...args) => execFileSync('git', ['-C', root, ...args],
  { encoding: 'utf8', timeout: 30_000 }).trim();
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');
let validator, source, schema, corpus, output, sourceCommit, beforeDigests;

async function inputDigests() {
  return Object.fromEntries(await Promise.all(INPUTS.map(async (path) =>
    [path, digest(await readFile(join(ROOT, path)))])));
}

before(async () => {
  sourceCommit = git(ROOT, 'rev-parse', 'HEAD');
  assert.equal(git(ROOT, 'status', '--porcelain', '--untracked-files=all', '--', ...INPUTS), '');
  assert.equal(git(TOOL, 'rev-parse', 'HEAD'), TJSV_REVISION, 'wrong TJSV revision');
  assert.equal(git(TOOL, 'status', '--porcelain', '--untracked-files=no'), '', 'dirty TJSV source');
  beforeDigests = await inputDigests();
  validator = await import(pathToFileURL(join(TOOL, 'src/index.mjs')).href);
  source = await readFile(join(ROOT, INPUTS[0]), 'utf8');
  schema = JSON.parse(await readFile(join(ROOT, INPUTS[1]), 'utf8'));
  corpus = JSON.parse(await readFile(join(ROOT, INPUTS[2]), 'utf8'));
  assert.equal(corpus.authority, 'independent-fixture-corpus');
  assert.equal(corpus.model, 'IdempotencyRecord');
  assert(corpus.cases.length > 0, 'missing independent corpus');
  assert.equal(new Set(corpus.cases.map(({ id }) => id)).size, corpus.cases.length);
  for (const item of corpus.cases) {
    assert.match(item.id, /^[a-z0-9-]+$/);
    assert(['accept', 'reject'].includes(item.expect));
    assert(Object.hasOwn(item, 'value'));
  }
  await mkdir(join(ROOT, 'target/persistence-tjsv'), { recursive: true });
  output = await mkdtemp(join(ROOT, 'target/persistence-tjsv/run-'));
});

after(async () => {
  if (!output) return;
  const afterDigests = await inputDigests();
  assert.deepEqual(afterDigests, beforeDigests, 'contract inputs changed during tests');
  assert.equal(git(ROOT, 'rev-parse', 'HEAD'), sourceCommit);
  assert.equal(git(ROOT, 'status', '--porcelain', '--untracked-files=all', '--', ...INPUTS), '');
  await writeFile(join(output, 'provenance.json'), `${JSON.stringify({
    schema: 'ores.middleware.persistence-tjsv-provenance/v1',
    sourceCommit, validatorRepository: 'ORESoftware/typespec-json-schema-validator',
    validatorRevision: TJSV_REVISION, inputSha256: afterDigests,
    sourceIntegrity: 'verified',
    scope: 'IdempotencyRecord and IdempotencyStatus only; not native-runtime certification',
  }, null, 2)}\n`, { flag: 'wx' });
});

async function check(name, { tsp = source, authored = schema, cases = corpus.cases } = {}) {
  const dir = join(output, name);
  const authoredDir = join(dir, 'authored');
  await mkdir(authoredDir, { recursive: true });
  const typespec = join(authoredDir, 'main.tsp');
  const authoredSchema = join(authoredDir, 'schema.json');
  await writeFile(typespec, tsp, { flag: 'wx' });
  await writeFile(authoredSchema, JSON.stringify(authored), { flag: 'wx' });
  const instances = join(dir, 'instances');
  for (const item of cases) {
    const location = join(instances, 'IdempotencyRecord', item.expect === 'accept' ? 'valid' : 'invalid');
    await mkdir(location, { recursive: true });
    await writeFile(join(location, `${item.id}.json`), JSON.stringify(item.value), { flag: 'wx' });
  }
  const report = await validator.runCheck({ typespec, authoredSchema, instances,
    outputDir: join(dir, 'witness'), tspBin: join(TOOL, 'node_modules/.bin/tsp'),
    maxFindings: 1000, maxProbes: 128, probes: true, formatAssertion: true,
    int64Strategy: 'number', sealObjectSchemas: true,
  });
  await validator.writeReport(join(dir, 'report.json'), report);
  assert.equal(report.toolchain.typespecCompiler.available, true, 'compiler must actually run');
  assert.equal(report.counts.findingsTruncated, false, 'truncated findings are not sufficient evidence');
  return report;
}

function assertPass(report) {
  assert.equal(report.schema, 'ores.typespec-json-schema-validator.report/v1');
  assert.equal(report.status, 'passed', JSON.stringify(report.findings));
  assert.equal(report.zeroUnexplainedFindings, true);
  assert.deepEqual(report.findings, []);
  for (const key of ['findings', 'structuralFindings', 'differentialFindings', 'typespecOutOfScopeDeclarations']) {
    assert.equal(report.counts[key], 0, key);
  }
  for (const key of ['typespecDeclarations', 'authoredDeclarations', 'generatedDeclarations']) {
    assert.equal(report.counts[key], 2, key);
  }
  assert.deepEqual(report.declarationMap.map(({ authored }) => authored).sort(),
    ['IdempotencyRecord', 'IdempotencyStatus']);
  assert.equal(report.differential.declarations.length, 2);
  for (const declaration of report.differential.declarations) {
    assert(declaration.probes > 0, 'unexercised declaration');
    assert.equal(declaration.divergences, 0);
    assert.equal(declaration.refusals, 0);
  }
  const summary = report.differential.summary;
  assert.equal(summary.comparedDeclarations, 2);
  assert.equal(summary.corpusInstances, corpus.cases.length);
  assert.equal(summary.formatAssertion, true);
  assert.equal(summary.divergences, 0);
  assert.equal(summary.refusals, 0);
  assert(summary.probesEvaluated > corpus.cases.length);
  assert.equal(summary.agreements, summary.probesEvaluated);
}

function assertBlocked(report) {
  assert.equal(report.status, 'stopped_for_evaluation', JSON.stringify(report.findings));
  assert.equal(report.zeroUnexplainedFindings, false);
  assert(report.counts.findings > 0);
}

test('actual pinned TJSV admits both persistence declarations and the independent corpus', async () => {
  assertPass(await check('positive'));
});

test('the additive closure annotation preserves the existing JSON wire contract', () => {
  const legacy = structuredClone(schema);
  delete legacy.$defs.IdempotencyRecord.unevaluatedProperties;
  const makeValidator = (document) => {
    const ajv = new Ajv2020({ allErrors: true, strict: false });
    addFormats(ajv);
    return ajv.compile(document);
  };
  const oldValidate = makeValidator(legacy);
  const newValidate = makeValidator(schema);
  for (const item of corpus.cases) {
    assert.equal(oldValidate(item.value), item.expect === 'accept', `${item.id}: legacy`);
    assert.equal(newValidate(item.value), item.expect === 'accept', `${item.id}: amended`);
  }
  const model = schema.$defs.IdempotencyRecord;
  assert.equal(model.additionalProperties, false, 'legacy translators still require this');
  assert.equal(model.unevaluatedProperties, false);
  // This proof is intentionally limited to the current flat object. Composition
  // would require a new semantic review, not a blanket keyword substitution.
  for (const keyword of ['allOf', 'anyOf', 'oneOf', '$ref', 'patternProperties', 'if', 'then', 'else']) {
    assert(!Object.hasOwn(model, keyword), `re-review closure semantics before adding ${keyword}`);
  }
});

test('TJSV does not ignore one-sided executable SQL table metadata drift', async () => {
  const tsp = source.replace('table: "middleware_idempotency"', 'table: "wrong_table"');
  assert.notEqual(tsp, source);
  const report = await check('sql-table-drift', { tsp });
  assertBlocked(report);
  assert(report.findings.some(({ pointer }) => pointer.endsWith('/x-ores-sql/table')));
  assert.equal(report.counts.differentialFindings, 0, 'table names are structural, not JSON instance behavior');
});

test('TJSV does not ignore one-sided tenant uniqueness metadata drift', async () => {
  const authored = structuredClone(schema);
  authored.$defs.IdempotencyRecord['x-ores-sql'].unique = [['idempotencyKey']];
  const report = await check('sql-unique-drift', { authored });
  assertBlocked(report);
  assert(report.findings.some(({ pointer }) => pointer.includes('/x-ores-sql/unique')));
});

test('TJSV rejects a one-sided tenant scalar change using real instance verdicts', async () => {
  const authored = structuredClone(schema);
  authored.$defs.IdempotencyRecord.properties.tenantId = { type: 'integer' };
  const report = await check('tenant-scalar-drift', { authored });
  assertBlocked(report);
  assert(report.differential.summary.divergences > 0);
});

test('TJSV rejects opening the authored record to unknown properties', async () => {
  const authored = structuredClone(schema);
  delete authored.$defs.IdempotencyRecord.additionalProperties;
  delete authored.$defs.IdempotencyRecord.unevaluatedProperties;
  const report = await check('unknown-property-drift', { authored });
  assertBlocked(report);
  assert(report.differential.summary.divergences > 0);
});

test('TJSV retains independent corpus expectations even when both schema lanes agree', async () => {
  const cases = structuredClone(corpus.cases);
  cases.find(({ id }) => id === 'valid-minimal').expect = 'reject';
  const report = await check('contradictory-corpus', { cases });
  assertBlocked(report);
  assert.equal(report.counts.structuralFindings, 0);
  assert(report.counts.differentialFindings > 0);
});
