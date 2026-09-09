import assert from 'node:assert/strict';
import { test } from 'node:test';
import { spawnSync } from 'node:child_process';
import {
  cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync,
  rmSync, symlinkSync, writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  assertSuccessfulReceipt, childEnvironment, DECLARATIONS, runDocsGate,
  snapshotInputs, TJSV_REV,
} from '../scripts/check-tjsv-docs.mjs';

// These are runner/receipt unit tests, NOT compiler-backed TJSV or adapter tests.
const sourceRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const count = snapshotInputs(sourceRoot).corpusInstances;
function receipt() {
  return {
    schema: 'ores.typespec-json-schema-validator.report/v1',
    runId: 'a'.repeat(64), status: 'passed', zeroUnexplainedFindings: true, findings: [],
    counts: { findings: 0, differentialFindings: 0, emittedFindings: 0, structuralFindings: 0,
      typespecOutOfScopeDeclarations: 0, findingsTruncated: false,
      authoredDeclarations: 4, generatedDeclarations: 4, typespecDeclarations: 4 },
    coverage: { differentialInstanceValidation: true, directDeclarationInventory: true,
      sourceMutationCheck: true, typespecGeneratedJsonSchemaComparison: true,
      outOfScopeTypeSpecDeclarations: [] },
    differential: {
      summary: { comparedDeclarations: 4, probesEvaluated: 100, agreements: 100,
        divergences: 0, refusals: 0, corpusInstances: count, behaviorallyIndistinguishableDeclarations: 4 },
      declarations: DECLARATIONS.map((name) => ({
        typespec: `Ores.Middleware.Docs.${name}`, authored: name, generated: name,
        probes: 25, divergences: 0, refusals: 0, behaviorallyIndistinguishable: true,
      })),
    },
  };
}
function sandbox(t) {
  const root = mkdtempSync(join(tmpdir(), 'ores-tjsv-gate-test-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  cpSync(join(sourceRoot, 'contracts'), join(root, 'contracts'), { recursive: true });
  mkdirSync(join(root, 'tmp/tjsv/bin'), { recursive: true });
  writeFileSync(join(root, 'tmp/tjsv/bin/typespec-json-schema-validator.mjs'), '// test-only placeholder\n');
  return root;
}
const success = (stdout = '') => ({ status: 0, signal: null, stdout });
function fakeTransport(root, onTjsv, onGit) {
  return (command, args, options) => {
    assert.equal(options.shell, false);
    assert.equal(options.timeout, 120_000);
    assert.equal(options.killSignal, 'SIGKILL');
    assert.equal(options.maxBuffer, 1024 * 1024);
    assert(!Object.hasOwn(options.env, 'NODE_OPTIONS'));
    assert(!Object.hasOwn(options.env, 'TSJSV_PROBES'));
    if (command === 'git') {
      const replacement = onGit?.(args);
      if (replacement) return replacement;
      return success(args.includes('--show-toplevel') ? join(root, 'tmp/tjsv')
        : args.includes('HEAD') && args.includes('rev-parse') ? TJSV_REV : '');
    }
    assert.equal(command, process.execPath);
    assert.equal(args[1], 'check');
    assert(args.includes('--probes=true'));
    assert(args.includes('--seal-object-schemas=false'));
    const reportPath = args.find((arg) => arg.startsWith('--report=')).slice('--report='.length);
    assert(!existsSync(reportPath), 'each invocation must use a fresh report');
    return onTjsv(reportPath, args, options);
  };
}
function writeReceipt(path, value = receipt()) { writeFileSync(path, JSON.stringify(value)); }
function expectFailed(root, execute) {
  assert.throws(() => runDocsGate(root, execute), /tjsv-docs-gate-failed/);
  const dir = readdirSync(join(root, 'target/tjsv')).sort().at(-1);
  assert.equal(JSON.parse(readFileSync(join(root, 'target/tjsv', dir, 'gate.json'))).status, 'failed');
}

test('valid fully-covered receipt is admitted', () => assertSuccessfulReceipt(receipt(), count));
const mutations = {
  'missing finding counts': (r) => { delete r.counts; },
  'truncated findings': (r) => { r.counts.findingsTruncated = true; },
  'hidden structural finding': (r) => { r.counts.structuralFindings = 1; },
  'missing source declaration': (r) => { r.counts.typespecDeclarations = 3; },
  'out-of-scope declaration count': (r) => { r.counts.typespecOutOfScopeDeclarations = 1; },
  'missing coverage': (r) => { delete r.coverage; },
  'skipped source mutation check': (r) => { r.coverage.sourceMutationCheck = false; },
  'uncompared declaration': (r) => { r.coverage.outOfScopeTypeSpecDeclarations = ['Other']; },
  'wrong TypeSpec namespace': (r) => { r.differential.declarations[0].typespec = 'Other.DocsAction'; },
  'crossed declaration mapping': (r) => { [r.differential.declarations[0].generated, r.differential.declarations[1].generated] = [r.differential.declarations[1].generated, r.differential.declarations[0].generated]; },
  'inconsistent agreement total': (r) => { r.differential.summary.agreements -= 1; },
  'inconsistent row probe total': (r) => { r.differential.declarations[0].probes += 1; },
  'incomplete agreeing declaration count': (r) => { r.differential.summary.behaviorallyIndistinguishableDeclarations = 3; },
  'wrong schema': (r) => { r.schema = 'other'; },
  'missing run identity': (r) => { delete r.runId; },
  'malformed run identity': (r) => { r.runId = 'x'.repeat(64); },
  'stopped parity': (r) => { r.status = 'stopped_for_evaluation'; },
  'failed parity': (r) => { r.status = 'failed'; },
  'unexplained flag false': (r) => { r.zeroUnexplainedFindings = false; },
  'findings not empty': (r) => { r.findings = [{ ruleId: 'drift' }]; },
  'missing findings': (r) => { delete r.findings; },
  'missing differential': (r) => { delete r.differential; },
  'disabled differential': (r) => { r.differential.disabled = true; },
  'malformed disabled field': (r) => { r.differential.disabled = false; },
  'null summary': (r) => { r.differential.summary = null; },
  'no probes': (r) => { r.differential.summary.probesEvaluated = 0; },
  'string probe count': (r) => { r.differential.summary.probesEvaluated = '100'; },
  'no agreements': (r) => { r.differential.summary.agreements = 0; },
  'divergent evidence': (r) => { r.differential.summary.divergences = 1; },
  'refused validation': (r) => { r.differential.summary.refusals = 1; },
  'incomplete declaration count': (r) => { r.differential.summary.comparedDeclarations = 3; },
  'incomplete corpus count': (r) => { r.differential.summary.corpusInstances -= 1; },
  'missing declaration': (r) => { r.differential.declarations.pop(); },
  'duplicate declaration': (r) => { r.differential.declarations[0] = r.differential.declarations[1]; },
  'wrong generated identity': (r) => { r.differential.declarations[0].generated = 'Other'; },
  'zero declaration probes': (r) => { r.differential.declarations[0].probes = 0; },
  'declaration refusal': (r) => { r.differential.declarations[0].refusals = 1; },
  'declaration divergence': (r) => { r.differential.declarations[0].divergences = 1; },
  'unproven declaration': (r) => { r.differential.declarations[0].behaviorallyIndistinguishable = false; },
};
for (const [name, mutate] of Object.entries(mutations)) test(`rejects ${name}`, () => {
  const r = receipt(); mutate(r); assert.throws(() => assertSuccessfulReceipt(r, count));
});
test('rejects absent expected corpus', () => assert.throws(() => assertSuccessfulReceipt(receipt(), 0)));
test('does not forward credentials or validator/Node overrides', () => {
  assert.deepEqual(childEnvironment({ PATH: '/bin', HOME: '/tmp', GH_TOKEN: 'test-only',
    LINEAR_API_KEY: 'test-only', NODE_OPTIONS: '--require=other', TSJSV_MAPPING: 'other' }),
  { PATH: '/bin', HOME: '/tmp' });
});
test('runs pinned TJSV with complete inputs and immutable authorities', (t) => {
  const root = sandbox(t); const before = snapshotInputs(root);
  const result = runDocsGate(root, fakeTransport(root, (path) => { writeReceipt(path); return success(); }));
  assert.equal(result.status, 'passed'); assert.equal(result.runtimeEvidence, 'not-evaluated');
  assert.deepEqual(snapshotInputs(root), before);
});
for (const [name, value] of Object.entries({
  'exit 2': { status: 2 }, 'exit 3': { status: 3 }, 'exit 1': { status: 1 },
  'signal': { status: null, signal: 'SIGKILL' }, 'launch error': { error: new Error('ENOENT') },
  'timeout': { status: null, error: Object.assign(new Error('timeout'), { code: 'ETIMEDOUT' }) },
})) test(`rejects ${name} even with a passing receipt`, (t) => {
  const root = sandbox(t);
  expectFailed(root, fakeTransport(root, (path) => { writeReceipt(path); return value; }));
});
test('rejects missing fresh receipt despite an older passing receipt', (t) => {
  const root = sandbox(t);
  mkdirSync(join(root, 'target/tjsv/old'), { recursive: true });
  writeReceipt(join(root, 'target/tjsv/old/parity.json'));
  assert.throws(() => runDocsGate(root, fakeTransport(root, () => success())), /tjsv-docs-gate-failed/);
});
test('rejects malformed report', (t) => {
  const root = sandbox(t);
  expectFailed(root, fakeTransport(root, (path) => { writeFileSync(path, '{'); return success(); }));
});
test('rejects changed authority bytes', (t) => {
  const root = sandbox(t);
  expectFailed(root, fakeTransport(root, (path) => {
    writeReceipt(path); writeFileSync(join(root, 'contracts/docs-serving.tsp'), '// changed\n'); return success();
  }));
});
test('rejects unexpected upstream revision before running TJSV', (t) => {
  const root = sandbox(t);
  expectFailed(root, fakeTransport(root, () => assert.fail('must not execute'),
    (args) => args.includes('rev-parse') && args.at(-1) === 'HEAD' ? success('b'.repeat(40)) : null));
});
test('rejects dirty upstream tracked content', (t) => {
  const root = sandbox(t);
  expectFailed(root, fakeTransport(root, () => assert.fail('must not execute'),
    (args) => args.includes('diff') ? { status: 1 } : null));
});
test('rejects parent repository mistaken for upstream checkout', (t) => {
  const root = sandbox(t);
  expectFailed(root, fakeTransport(root, () => assert.fail('must not execute'),
    (args) => args.includes('--show-toplevel') ? success(root) : null));
});
test('rejects symlinked output directory without writing outside checkout', (t) => {
  const root = sandbox(t); const external = mkdtempSync(join(tmpdir(), 'ores-tjsv-outside-'));
  t.after(() => rmSync(external, { recursive: true, force: true }));
  symlinkSync(external, join(root, 'target'), 'dir');
  assert.throws(() => runDocsGate(root, () => assert.fail('must not execute')), /symlink-not-allowed/);
  assert.deepEqual(readdirSync(external), []);
});
test('rejects unknown corpus declaration', (t) => {
  const root = sandbox(t); mkdirSync(join(root, 'contracts/tjsv-instances/docs-serving/Other'));
  assert.throws(() => snapshotInputs(root), /unexpected-corpus-declaration/);
});
test('rejects empty expectation lane', (t) => {
  const root = sandbox(t); const lane = join(root, 'contracts/tjsv-instances/docs-serving/DocsAction/valid');
  for (const name of readdirSync(lane)) rmSync(join(lane, name));
  assert.throws(() => snapshotInputs(root), /empty-expectation-lane/);
});
test('rejects fixture symlink', (t) => {
  const root = sandbox(t); const fixture = join(root, 'contracts/tjsv-instances/docs-serving/DocsAction/valid/pass.json');
  rmSync(fixture); symlinkSync(join(root, 'contracts/docs-serving.schema.json'), fixture);
  assert.throws(() => snapshotInputs(root), /symlink-not-allowed/);
});
test('real CLI rejects supplied flags rather than creating an independent parser', () => {
  const result = spawnSync(process.execPath, [join(sourceRoot, 'scripts/check-tjsv-docs.mjs'), '--probes=false'], { encoding: 'utf8' });
  assert.equal(result.status, 1); assert.equal(JSON.parse(result.stderr).status, 'failed');
});

test('workflow and runner use the same immutable TJSV revision', () => {
  const workflow = readFileSync(join(sourceRoot, '.github/workflows/tjsv-docs-serving.yml'), 'utf8');
  assert(workflow.includes(`ref: ${TJSV_REV}`));
  assert(!workflow.includes('continue-on-error'));
  assert(!workflow.includes('pull_request_target'));
});
test('workflow rebuilds only the native f2e dependency after script-disabled install', () => {
  const workflow = readFileSync(join(sourceRoot, '.github/workflows/tjsv-docs-serving.yml'), 'utf8');
  const install = workflow.indexOf('npm ci --prefix tmp/tjsv --ignore-scripts');
  const rebuild = workflow.indexOf('npm rebuild @oresoftware/f2e --ignore-scripts=false');
  const gate = workflow.indexOf('          npm run docs-serving:tjsv\n');
  assert(install > 0 && rebuild > install && gate > rebuild);
  assert(workflow.includes('test -s node_modules/@oresoftware/f2e/clients/nodejs/build/Release/flags2env.node'));
});


test('workflow installs the root decorator dependency before compilation', () => {
  const workflow = readFileSync(join(sourceRoot, '.github/workflows/tjsv-docs-serving.yml'), 'utf8');
  const install = workflow.indexOf('run: npm ci --ignore-scripts --no-audit --no-fund');
  assert(install > 0 && install < workflow.indexOf('          npm run docs-serving:tjsv\n'));
});
