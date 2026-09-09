import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { assertMatrix, assertPassingReport, VALIDATOR_PACKAGE } from '../scripts/tjsv-evidence.mjs';
import { checkOptions } from '../scripts/tjsv-check.mjs';

const matrix = JSON.parse(await readFile(new URL('../contracts/tjsv.matrix.json', import.meta.url), 'utf8'));
const copy = value => structuredClone(value);
const digest = 'a'.repeat(64);
function passing() {
  return {
    schema: 'ores.typespec-json-schema-validator.report/v1', runId: digest,
    status: 'passed', zeroUnexplainedFindings: true,
    authorities: { precedence: 'none', typespec: { authority: 'independently-authored',
      generatedJsonSchemaRole: 'comparison-evidence-only' }, jsonSchema: { authority: 'independently-authored' } },
    configuration: { mode: 'check', differential: { enabled: true, formatAssertion: true, maxProbesPerDeclarationPerLane: 128 } },
    coverage: { directDeclarationInventory: true, typespecGeneratedJsonSchemaComparison: true,
      differentialInstanceValidation: true, sourceMutationCheck: true },
    toolchain: { validator: { name: VALIDATOR_PACKAGE }, typespecCompiler: { available: true } },
    inputs: Object.fromEntries(['typespec', 'authoredJsonSchema', 'generatedJsonSchema'].map(key =>
      [key, { digest, files: [{ relativePath: key, sha256: digest }] }])),
    counts: { typespecDeclarations: 1, authoredDeclarations: 1, generatedDeclarations: 1,
      structuralFindings: 0, differentialFindings: 0, findings: 0, emittedFindings: 0, findingsTruncated: false },
    declarationMap: [{ typespec: 'Envelope', authored: 'Envelope', generated: 'Envelope' }], findings: [],
    differential: { summary: { comparedDeclarations: 1, behaviorallyIndistinguishableDeclarations: 1,
      probesEvaluated: 4, agreements: 4, divergences: 0, refusals: 0, formatAssertion: true },
      declarations: [{ typespec: 'Envelope', authored: 'Envelope', generated: 'Envelope',
        probes: 4, divergences: 0, refusals: 0, behaviorallyIndistinguishable: true }] },
  };
}

test('all five existing independently-authored contract surfaces are in the matrix', () => {
  assertMatrix(matrix);
  assert.deepEqual(matrix.lanes.map(x => x.id).sort(),
    ['docs-serving', 'function-bodies', 'middleware', 'persistence', 'rate-limit-v2']);
});
test('complete, non-empty compiler and differential evidence passes', () => assertPassingReport(passing()));

const reportMutants = {
  'missing report': () => null,
  'exit-zero without evidence': () => ({ status: 'passed' }),
  'wrong schema': r => { r.schema = 'other/v1'; },
  'stopped for evaluation': r => { r.status = 'stopped_for_evaluation'; },
  'failed execution': r => { r.status = 'failed'; },
  'unexplained findings flag': r => { r.zeroUnexplainedFindings = false; },
  'bad run digest': r => { r.runId = 'latest'; },
  'generated authority elevation': r => { r.authorities.precedence = 'typespec'; },
  'wrong witness role': r => { r.authorities.typespec.generatedJsonSchemaRole = 'authority'; },
  'compare-only evidence': r => { r.configuration.mode = 'compare'; },
  'disabled differential': r => { r.configuration.differential.enabled = false; },
  'annotation-only formats': r => { r.configuration.differential.formatAssertion = false; },
  'zero probe budget': r => { r.configuration.differential.maxProbesPerDeclarationPerLane = 0; },
  'wrong validator': r => { r.toolchain.validator.name = 'local-regex-scanner'; },
  'missing compiler': r => { r.toolchain.typespecCompiler.available = false; },
  'structural mismatch': r => { r.counts.structuralFindings = 1; },
  'differential mismatch': r => { r.counts.differentialFindings = 1; },
  'truncated findings': r => { r.counts.findingsTruncated = true; },
  'finding hidden by counts': r => { r.findings.push({ ruleId: 'drift' }); },
  'empty declaration map': r => { r.declarationMap = []; },
  'uncovered authored declaration': r => { r.counts.authoredDeclarations = 2; },
  'disabled evidence summary': r => { r.differential.disabled = true; },
  'empty summary': r => { r.differential.summary = null; },
  'zero executed probes': r => { r.differential.summary.probesEvaluated = 0; },
  'validator refusal': r => { r.differential.summary.refusals = 1; },
  'runtime divergence': r => { r.differential.summary.divergences = 1; },
  'missing declaration evidence': r => { r.differential.declarations = []; },
  'unexecuted declaration': r => { r.differential.declarations[0].probes = 0; },
  'wrong declaration identity': r => { r.differential.declarations[0].authored = 'Other'; },
  'false indistinguishability': r => { r.differential.declarations[0].behaviorallyIndistinguishable = false; },
  'inconsistent probe accounting': r => { r.differential.declarations[0].probes = 5; },
};
for (const field of ['directDeclarationInventory', 'typespecGeneratedJsonSchemaComparison',
  'differentialInstanceValidation', 'sourceMutationCheck']) {
  reportMutants[`missing ${field}`] = r => { delete r.coverage[field]; };
}
for (const input of ['typespec', 'authoredJsonSchema', 'generatedJsonSchema']) {
  reportMutants[`empty ${input} evidence`] = r => { r.inputs[input].files = []; };
  reportMutants[`bad ${input} digest`] = r => { r.inputs[input].digest = 'bad'; };
}
for (const [name, mutate] of Object.entries(reportMutants)) {
  test(`rejects ${name}`, () => {
    const report = passing();
    const replacement = mutate(report);
    assert.throws(() => assertPassingReport(replacement === undefined ? report : replacement));
  });
}

const matrixMutants = {
  'empty matrix': m => { m.lanes = []; },
  'mutable tool revision': m => { m.validator.revision = 'main'; },
  'wrong repository': m => { m.validator.repository = 'other/tool'; },
  'unknown bypass switch': m => { m.continueOnError = true; },
  'lane bypass switch': m => { m.lanes[0].skip = true; },
  'duplicate lane': m => { m.lanes.push(copy(m.lanes[0])); },
  'parent traversal': m => { m.lanes[0].typespec = 'contracts/../secrets.tsp'; },
  'absolute input': m => { m.lanes[0].typespec = '/tmp/source.tsp'; },
  'backslash traversal': m => { m.lanes[0].typespec = 'contracts/..\\source.tsp'; },
  'shell-like lane identifier': m => { m.lanes[0].id = 'a; exit 0'; },
  'empty path segment': m => { m.lanes[0].typespec = 'contracts//source.tsp'; },
  'NUL path': m => { m.lanes[0].typespec = 'contracts/\0source.tsp'; },
};
for (const [name, mutate] of Object.entries(matrixMutants)) {
  test(`matrix rejects ${name}`, () => { const value = copy(matrix); mutate(value); assert.throws(() => assertMatrix(value)); });
}
test('compiler invocation preserves independent inputs and fixes strict settings', () => {
  const options = checkOptions('/repo/contracts/source.tsp', '/repo/contracts/authority.json', '/repo/target/witness', '/tool');
  assert.equal(options.probes, true);
  assert.equal(options.formatAssertion, true);
  assert.equal(options.sealObjectSchemas, true);
  assert.equal(options.int64Strategy, 'number');
  assert.equal(options.maxProbes, 128);
  assert.equal(options.tspBin, '/tool/node_modules/.bin/tsp');
  assert.equal(options.mapping, undefined);
  assert.notEqual(options.typespec, options.authoredSchema);
});
