import assert from 'node:assert/strict';
import { test } from 'node:test';
import { execFileSync } from 'node:child_process';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import Ajv2020 from 'ajv/dist/2020.js';
import { declarations, docsPeerCases } from './docs-serving-peer-cases.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
const baselineRevision = '6183cc877d6c058349adc733d325297c07d1c063';
const validatorRevision = '4473504c4c9d2831d825919f70c03994d8ce01d2';
const sourcePaths = ['contracts/docs-serving.tsp', 'contracts/docs-serving.schema.json',
  'tests/docs-serving-peer-cases.mjs', 'tests/docs-serving-peers.test.mjs',
  '.github/workflows/docs-serving-peer-contract.yml', 'package.json', 'package-lock.json'];
const git = (cwd, ...args) => execFileSync('git', ['-C', cwd, ...args], { encoding: 'utf8', timeout: 30_000 }).trim();
const sourceClean = () => assert.equal(git(root, 'status', '--porcelain', '--untracked-files=all', '--', ...sourcePaths), '', 'source closure is not committed');
const json = async (path, value) => writeFile(path, `${JSON.stringify(value, null, 2)}\n`, { flag: 'wx' });

function validators(schema, old = false) {
  const ajv = new Ajv2020({ strict: true, allErrors: true });
  ajv.addSchema(schema);
  return Object.fromEntries(declarations.map(name => [name,
    old && name === 'DocsHeaders' ? ajv.compile(schema.$defs.DocsDecision.properties.headers)
      : ajv.compile({ $ref: `${schema.$id}#/$defs/${name}` }),
  ]));
}

function assertPassing(report, corpusCount) {
  assert.equal(report.status, 'passed');
  assert.equal(report.zeroUnexplainedFindings, true);
  assert.equal(report.toolchain.typespecCompiler.available, true);
  for (const field of ['findings', 'structuralFindings', 'differentialFindings']) assert.equal(report.counts[field], 0, field);
  assert.equal(report.counts.findingsTruncated, false);
  assert.deepEqual(report.findings, []);
  for (const field of ['typespecDeclarations', 'authoredDeclarations', 'generatedDeclarations']) assert.equal(report.counts[field], declarations.length, field);
  assert.equal(report.counts.typespecOutOfScopeDeclarations, 0);
  for (const field of ['authored', 'generated']) assert.deepEqual(report.declarationMap.map(x => x[field]).sort(), declarations);
  assert.deepEqual(report.declarationMap.map(x => x.typespec).sort(), declarations.map(x => `Ores.Middleware.Docs.${x}`));
  const summary = report.differential.summary;
  assert.equal(summary.comparedDeclarations, declarations.length);
  assert.equal(summary.behaviorallyIndistinguishableDeclarations, declarations.length);
  assert.equal(summary.corpusInstances, corpusCount);
  assert.equal(summary.divergences, 0);
  assert.equal(summary.refusals, 0);
  assert.equal(summary.agreements, summary.probesEvaluated);
  assert(summary.probesEvaluated >= corpusCount);
  assert.deepEqual(report.differential.declarations.map(x => x.authored).sort(), declarations);
  for (const row of report.differential.declarations) {
    assert(row.probes > 0);
    assert.equal(row.divergences, 0);
    assert.equal(row.refusals, 0);
  }
}

test('docs-serving independently authored peers retain v1 semantics and pass actual pinned TJSV', { timeout: 180_000 }, async t => {
  sourceClean();
  const sourceCommit = git(root, 'rev-parse', 'HEAD');
  if (process.env.EXPECTED_SOURCE_SHA) assert.equal(sourceCommit, process.env.EXPECTED_SOURCE_SHA);
  const toolRoot = join(root, 'target/tools/tjsv');
  assert.equal(git(toolRoot, 'rev-parse', 'HEAD'), validatorRevision, 'TJSV pin changed');
  assert.equal(git(toolRoot, 'status', '--porcelain', '--untracked-files=no'), '', 'TJSV source is dirty');
  const validator = await import(pathToFileURL(join(toolRoot, 'src/index.mjs')).href);
  assert.equal(typeof validator.runCheck, 'function');
  const current = JSON.parse(await readFile(join(root, 'contracts/docs-serving.schema.json'), 'utf8'));
  const baseline = JSON.parse(git(root, 'show', `${baselineRevision}:contracts/docs-serving.schema.json`));
  const before = validators(baseline, true);
  const after = validators(current);
  const cases = docsPeerCases();
  assert.equal(new Set(cases.map(x => `${x.declaration}/${x.id}`)).size, cases.length);
  assert.deepEqual(Object.keys(current.$defs).sort(), declarations);
  for (const name of declarations) {
    assert(cases.some(x => x.declaration === name && x.valid));
    assert(cases.some(x => x.declaration === name && !x.valid));
  }
  // Child failures remain fatal and cannot produce a passing receipt.
  let semanticFailures = 0;
  for (const item of cases) {
    await t.test(`${item.declaration}/${item.id}`, () => {
      try {
        assert.equal(before[item.declaration](item.value), item.valid, 'baseline disagrees with independent expectation');
        assert.equal(after[item.declaration](item.value), item.valid, 'reconciled authority disagrees with independent expectation');
      } catch (error) { semanticFailures++; throw error; }
    });
  }
  assert.equal(semanticFailures, 0, 'independent semantic cases failed');
  const parent = join(root, 'target/docs-serving-peers');
  await mkdir(parent, { recursive: true });
  const run = await mkdtemp(join(parent, 'run-'));
  const instances = join(run, 'instances');
  for (const item of cases) {
    const dir = join(instances, item.declaration, item.valid ? 'valid' : 'invalid');
    await mkdir(dir, { recursive: true });
    await json(join(dir, `${item.id}.json`), item.value);
  }
  const options = { typespec: join(root, 'contracts/docs-serving.tsp'),
    authoredSchema: join(root, 'contracts/docs-serving.schema.json'),
    outputDir: join(run, 'positive-witness'), instances,
    tspBin: join(toolRoot, 'node_modules/.bin/tsp'),
    maxFindings: 1000, maxProbes: 128, probes: true, formatAssertion: true,
    int64Strategy: 'number', sealObjectSchemas: true };
  const positive = await validator.runCheck(options);
  await validator.writeReport(join(run, 'positive.json'), positive);
  assertPassing(positive, cases.length);
  const driftSchema = structuredClone(current);
  driftSchema.$defs.DocsRequest.properties.method = { type: 'integer' };
  const driftPath = join(run, 'intentional-drift.schema.json');
  await json(driftPath, driftSchema);
  const negative = await validator.runCheck({ ...options, authoredSchema: driftPath, outputDir: join(run, 'negative-witness') });
  await validator.writeReport(join(run, 'negative.json'), negative);
  assert.equal(negative.status, 'stopped_for_evaluation');
  assert.equal(negative.zeroUnexplainedFindings, false);
  assert(negative.counts.structuralFindings > 0);
  assert(negative.differential.summary.divergences > 0);
  sourceClean();
  assert.equal(git(root, 'rev-parse', 'HEAD'), sourceCommit);
  assert.equal(git(toolRoot, 'rev-parse', 'HEAD'), validatorRevision);
  assert.equal(git(toolRoot, 'status', '--porcelain', '--untracked-files=no'), '');
  await json(join(run, 'receipt.json'), { schema: 'ores.docs-serving-peer-reconciliation/v1',
    status: 'passed', sourceCommit, validatorRevision, baselineRevision,
    independentCases: cases.length, declarations, positiveRunId: positive.runId,
    positive: positive.differential.summary, negativeRunId: negative.runId,
    intentionalDrift: 'rejected', sourceIntegrity: 'verified',
    limitations: 'Finite schema evidence only; native transport and full release gates remain required.' });
});
