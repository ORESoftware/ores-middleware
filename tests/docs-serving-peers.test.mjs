import assert from 'node:assert/strict';
import { test } from 'node:test';
import { execFileSync } from 'node:child_process';
import { cp, lstat, mkdir, mkdtemp, readFile, realpath, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import Ajv2020 from 'ajv/dist/2020.js';
import { declarations as caseSubjects, docsPeerCases } from './docs-serving-peer-cases.mjs';
import { TJSV_REV, DECLARATIONS, assertSuccessfulReceipt } from '../scripts/check-tjsv-docs.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
const baselineRevision = '6183cc877d6c058349adc733d325297c07d1c063';
const sourcePaths = ['contracts', 'scripts/check-tjsv-docs.mjs',
  'tests/docs-serving-peer-cases.mjs', 'tests/docs-serving-peers.test.mjs',
  '.github/workflows/docs-serving-peer-contract.yml', 'package.json', 'package-lock.json', '.gitignore'];
const git = (cwd, ...args) => execFileSync('git', ['-C', cwd, ...args], { encoding: 'utf8', timeout: 30_000 }).trim();
const sourceClean = () => assert.equal(git(root, 'status', '--porcelain', '--untracked-files=all', '--', ...sourcePaths), '', 'source closure is not committed');
const json = async (path, value) => writeFile(path, `${JSON.stringify(value, null, 2)}\n`, { flag: 'wx' });

function validators(schema) {
  const ajv = new Ajv2020({ strict: true, allErrors: true });
  ajv.addSchema(schema);
  return Object.fromEntries(caseSubjects.map(name => [name,
    name === 'DocsHeaders' ? ajv.compile(schema.$defs.DocsDecision.properties.headers)
      : ajv.compile({ $ref: `${schema.$id}#/$defs/${name}` }),
  ]));
}

async function pinnedTool(toolRoot) {
  assert.equal(await realpath(toolRoot), toolRoot, 'tool checkout was redirected');
  assert.equal(git(toolRoot, 'rev-parse', '--show-toplevel'), toolRoot, 'independent tool checkout required');
  assert.equal(git(toolRoot, 'rev-parse', 'HEAD'), TJSV_REV, 'TJSV pin changed');
  assert.equal(git(toolRoot, 'status', '--porcelain', '--untracked-files=no'), '', 'TJSV source is dirty');
}

// Every former standalone header case is retained as an isolated schema check
// AND has an enclosing wire case. DocsHeaders is not a public declaration on main.
function wireCases(cases) {
  assert.deepEqual([...caseSubjects].sort(), [...DECLARATIONS, 'DocsHeaders'].sort());
  const wire = cases.filter(item => item.declaration !== 'DocsHeaders');
  for (const item of cases.filter(item => item.declaration === 'DocsHeaders')) {
    const enclosing = wire.find(row => row.declaration === 'DocsDecision' && row.id === `headers-${item.id}`);
    assert(enclosing, `missing enclosing wire witness: ${item.id}`);
    assert.equal(enclosing.valid, item.valid);
    assert.deepEqual(enclosing.value.headers, item.value);
  }
  return wire;
}

const mutations = [
  ['method-scalar', schema => { schema.$defs.DocsRequest.properties.method = { type: 'integer' }; }],
  ['request-closure', schema => { schema.$defs.DocsRequest.additionalProperties = true; }],
  ['decision-closure', schema => { schema.$defs.DocsDecision.additionalProperties = true; }],
  ['status-width', schema => { schema.$defs.DocsDecision.properties.status.maximum = 65536; }],
  ['header-value-type', schema => { schema.$defs.DocsDecision.properties.headers.unevaluatedProperties = { type: 'integer' }; }],
  ['digest-pattern', schema => { delete schema.$defs.DocsRequest.properties.runtimeContractDigest.pattern; }],
];

function assertRejected(report, count, structural) {
  assert.equal(report.status, 'stopped_for_evaluation');
  assert.equal(report.zeroUnexplainedFindings, false);
  assert.equal(report.counts.findingsTruncated, false);
  assert.equal(report.differential.summary.corpusInstances, count);
  assert.equal(report.differential.summary.comparedDeclarations, DECLARATIONS.length);
  assert.equal(report.differential.summary.refusals, 0);
  if (structural) {
    assert(report.counts.structuralFindings > 0, 'mutation was not found structurally');
    assert(report.differential.summary.divergences > 0, 'mutation did not exercise a behavior difference');
  } else {
    assert.equal(report.counts.structuralFindings, 0, 'corpus control changed the peer authorities');
    assert(report.counts.differentialFindings > 0, 'contradictory expected verdict was not rejected');
  }
}

test('historical docs wire semantics, current pinned TJSV, and adversarial rejection', { timeout: 300_000 }, async t => {
  sourceClean();
  const sourceCommit = git(root, 'rev-parse', 'HEAD');
  if (process.env.EXPECTED_SOURCE_SHA) assert.equal(sourceCommit, process.env.EXPECTED_SOURCE_SHA);
  const toolRoot = join(root, 'tmp/tjsv');
  await pinnedTool(toolRoot);
  const validator = await import(pathToFileURL(join(toolRoot, 'src/index.mjs')).href);
  assert.equal(typeof validator.runCheck, 'function');
  const current = JSON.parse(await readFile(join(root, 'contracts/docs-serving.schema.json'), 'utf8'));
  const baseline = JSON.parse(git(root, 'show', `${baselineRevision}:contracts/docs-serving.schema.json`));
  const before = validators(baseline);
  const after = validators(current);
  const cases = docsPeerCases();
  const wire = wireCases(cases);
  assert.equal(cases.length, 133, 'historical independent corpus was lost');
  assert.equal(wire.length, 118, 'wire corpus was lost');
  assert.equal(new Set(cases.map(x => `${x.declaration}/${x.id}`)).size, cases.length);
  assert.deepEqual(Object.keys(current.$defs).sort(), DECLARATIONS);
  assert.deepEqual(current.oneOf, baseline.oneOf, 'root wire union changed');
  for (const name of caseSubjects) {
    assert(cases.some(x => x.declaration === name && x.valid));
    assert(cases.some(x => x.declaration === name && !x.valid));
  }
  let failures = 0;
  const checked = async (name, work) => t.test(name, async () => {
    try { await work(); } catch (error) { failures++; throw error; }
  });
  for (const item of cases) {
    await checked(`${item.declaration}/${item.id}`, () => {
      assert.equal(before[item.declaration](item.value), item.valid, 'baseline disagrees with independent expectation');
      assert.equal(after[item.declaration](item.value), item.valid, 'current authority disagrees with independent expectation');
    });
  }
  assert.equal(failures, 0, 'independent semantic cases failed');
  const parent = join(root, 'target/docs-serving-peers');
  await mkdir(parent, { recursive: true });
  assert.equal(await realpath(parent), parent, 'evidence destination redirected');
  assert(!(await lstat(parent)).isSymbolicLink());
  const run = await mkdtemp(join(parent, 'run-'));
  const instances = join(run, 'instances');
  for (const item of wire) {
    const dir = join(instances, item.declaration, item.valid ? 'valid' : 'invalid');
    await mkdir(dir, { recursive: true });
    await json(join(dir, `${item.id}.json`), item.value);
  }
  const options = {
    typespec: join(root, 'contracts/docs-serving.tsp'),
    authoredSchema: join(root, 'contracts/docs-serving.schema.json'),
    outputDir: join(run, 'positive-witness'), instances,
    tspBin: join(toolRoot, 'node_modules/.bin/tsp'),
    maxFindings: 1000, maxProbes: 128, probes: true,
    // Same explicit closure and scalar policy as the landed CLI gate. This
    // does not remove source constraints or waive any structural finding.
    formatAssertion: false, int64Strategy: 'string',
    polymorphicModelsStrategy: 'oneOf', sealObjectSchemas: false,
  };
  const positive = await validator.runCheck(options);
  await validator.writeReport(join(run, 'positive.json'), positive);
  assertSuccessfulReceipt(positive, wire.length);
  assert.equal(positive.toolchain.typespecCompiler.available, true);
  const controls = [];
  for (const [name, mutate] of mutations) {
    await checked(`actual-TJSV-rejects-${name}`, async () => {
      const altered = structuredClone(current);
      mutate(altered);
      const schemaPath = join(run, `${name}.schema.json`);
      await json(schemaPath, altered);
      const report = await validator.runCheck({ ...options,
        authoredSchema: schemaPath, outputDir: join(run, `${name}-witness`) });
      await validator.writeReport(join(run, `${name}.json`), report);
      assertRejected(report, wire.length, true);
      controls.push({ name, runId: report.runId, counts: report.counts, differential: report.differential.summary });
    });
  }
  await checked('actual-TJSV-rejects-contradictory-corpus-with-unchanged-peers', async () => {
    const badInstances = join(run, 'contradictory-instances');
    await cp(instances, badInstances, { recursive: true, errorOnExist: true, force: false });
    // This numeric method is invalid in BOTH authorities and already exists in
    // the invalid lane. Labelling it valid must not be silently deduplicated.
    await json(join(badInstances, 'DocsRequest/valid/contradictory-method.json'), { method: 1, path: '/api-docs' });
    const report = await validator.runCheck({ ...options, instances: badInstances,
      outputDir: join(run, 'contradictory-witness') });
    await validator.writeReport(join(run, 'contradictory.json'), report);
    assertRejected(report, wire.length + 1, false);
    controls.push({ name: 'contradictory-corpus', runId: report.runId, counts: report.counts, differential: report.differential.summary });
  });
  assert.equal(failures, 0, 'at least one required rejection control failed');
  assert.equal(controls.length, mutations.length + 1);
  sourceClean();
  assert.equal(git(root, 'rev-parse', 'HEAD'), sourceCommit);
  await pinnedTool(toolRoot);
  await json(join(run, 'receipt.json'), {
    schema: 'ores.docs-serving-peer-reconciliation/v2', status: 'passed',
    sourceCommit, validatorRevision: TJSV_REV, baselineRevision,
    independentCases: cases.length, headerSubschemaChecks: cases.length - wire.length,
    wireCorpusInstances: wire.length, declarations: DECLARATIONS,
    positiveRunId: positive.runId, positive: positive.differential.summary,
    controls, sourceIntegrity: 'verified',
    limitations: 'Finite schema evidence only; native transport and full release gates remain required.',
  });
});
