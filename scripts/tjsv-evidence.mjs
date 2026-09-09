import assert from 'node:assert/strict';

export const VALIDATOR_REPOSITORY = 'ORESoftware/typespec-json-schema-validator';
export const VALIDATOR_PACKAGE = '@oresoftware/typespec-json-schema-validator';
const SHA256 = /^[a-f0-9]{64}$/u;

function object(value, label) {
  assert(value !== null && typeof value === 'object' && !Array.isArray(value), label);
}
function exactKeys(value, keys, label) {
  object(value, label);
  assert.deepEqual(Object.keys(value).sort(), [...keys].sort(), `${label}: unknown or missing fields`);
}
function positive(value, label) {
  assert(Number.isSafeInteger(value) && value > 0, label);
}
function contractPath(value, label) {
  assert(typeof value === 'string' && value.startsWith('contracts/'), label);
  assert(!/[\\\s\x00-\x1f\x7f]/u.test(value), label);
  assert(value.split('/').every(part => part !== '' && part !== '.' && part !== '..'), label);
}

/** Reject skip switches, shell fragments, duplicate lanes, and mutable tool refs. */
export function assertMatrix(matrix) {
  exactKeys(matrix, ['schema', 'validator', 'lanes'], 'matrix');
  assert.equal(matrix.schema, 'ores.middleware.tjsv-matrix/v1');
  exactKeys(matrix.validator, ['repository', 'revision'], 'validator');
  assert.equal(matrix.validator.repository, VALIDATOR_REPOSITORY);
  assert.match(matrix.validator.revision, /^[a-f0-9]{40}$/u);
  assert(Array.isArray(matrix.lanes) && matrix.lanes.length > 0, 'empty matrix');
  const ids = new Set();
  const sources = new Set();
  for (const lane of matrix.lanes) {
    const keys = ['id', 'typespec', 'authoredSchema'];
    if (lane && Object.hasOwn(lane, 'corpus')) keys.push('corpus');
    exactKeys(lane, keys, 'lane');
    assert.match(lane.id, /^[a-z][a-z0-9-]{0,63}$/u);
    assert(!ids.has(lane.id), 'duplicate lane');
    assert(!sources.has(lane.typespec), 'duplicate TypeSpec input');
    ids.add(lane.id);
    sources.add(lane.typespec);
    contractPath(lane.typespec, 'TypeSpec path');
    contractPath(lane.authoredSchema, 'authored schema path');
    assert(lane.typespec.endsWith('.tsp'), 'TypeSpec entry must be explicit');
    assert.notEqual(lane.typespec, lane.authoredSchema, 'authorities must be independent');
    if (Object.hasOwn(lane, 'corpus')) {
      contractPath(lane.corpus, 'corpus path');
      assert(lane.corpus.endsWith('.json'), 'corpus must be an explicit JSON manifest');
      assert.notEqual(lane.corpus, lane.authoredSchema, 'corpus must be independent of the schema');
    }
  }
  return matrix;
}

/** An exit-zero/"passed" label alone is not cross-language contract evidence. */
export function assertPassingReport(report) {
  object(report, 'missing report');
  assert.equal(report.schema, 'ores.typespec-json-schema-validator.report/v1');
  assert.equal(report.status, 'passed');
  assert.equal(report.zeroUnexplainedFindings, true);
  assert.match(report.runId, SHA256);
  assert.equal(report.authorities?.precedence, 'none');
  assert.equal(report.authorities?.typespec?.authority, 'independently-authored');
  assert.equal(report.authorities?.typespec?.generatedJsonSchemaRole, 'comparison-evidence-only');
  assert.equal(report.authorities?.jsonSchema?.authority, 'independently-authored');
  assert.equal(report.configuration?.mode, 'check');
  assert.equal(report.configuration?.differential?.enabled, true);
  assert.equal(report.configuration?.differential?.formatAssertion, true);
  positive(report.configuration?.differential?.maxProbesPerDeclarationPerLane, 'probe budget');
  for (const field of ['directDeclarationInventory', 'typespecGeneratedJsonSchemaComparison',
    'differentialInstanceValidation', 'sourceMutationCheck']) {
    assert.equal(report.coverage?.[field], true, `missing coverage: ${field}`);
  }
  assert.equal(report.toolchain?.validator?.name, VALIDATOR_PACKAGE);
  assert.equal(report.toolchain?.typespecCompiler?.available, true, 'compiler evidence missing');
  for (const input of ['typespec', 'authoredJsonSchema', 'generatedJsonSchema']) {
    assert.match(report.inputs?.[input]?.digest, SHA256);
    assert(Array.isArray(report.inputs?.[input]?.files) && report.inputs[input].files.length > 0,
      `empty input evidence: ${input}`);
  }
  for (const field of ['typespecDeclarations', 'authoredDeclarations', 'generatedDeclarations']) {
    positive(report.counts?.[field], `empty declaration inventory: ${field}`);
  }
  for (const field of ['structuralFindings', 'differentialFindings', 'findings', 'emittedFindings']) {
    assert.equal(report.counts?.[field], 0, `unexplained findings: ${field}`);
  }
  assert.equal(report.counts?.findingsTruncated, false);
  assert.deepEqual(report.findings, []);
  assert(Array.isArray(report.declarationMap) && report.declarationMap.length > 0, 'missing mappings');
  const count = report.declarationMap.length;
  for (const field of ['typespecDeclarations', 'authoredDeclarations', 'generatedDeclarations']) {
    assert.equal(report.counts[field], count, `uncovered declarations: ${field}`);
  }
  assert.notEqual(report.differential?.disabled, true);
  const summary = report.differential?.summary;
  assert.equal(summary?.comparedDeclarations, count);
  assert.equal(summary?.behaviorallyIndistinguishableDeclarations, count);
  positive(summary?.probesEvaluated, 'zero executed probes');
  assert.equal(summary?.agreements, summary?.probesEvaluated);
  assert.equal(summary?.divergences, 0);
  assert.equal(summary?.refusals, 0);
  assert.equal(summary?.formatAssertion, true);
  assert(Array.isArray(report.differential?.declarations), 'missing differential declarations');
  assert.equal(report.differential.declarations.length, count);
  const identities = new Set(report.declarationMap.map(item => JSON.stringify([item.typespec, item.generated, item.authored])));
  assert.equal(identities.size, count, 'duplicate declaration mappings');
  let probes = 0;
  for (const entry of report.differential.declarations) {
    const identity = JSON.stringify([entry.typespec, entry.generated, entry.authored]);
    assert(identities.delete(identity), 'missing or duplicate differential declaration');
    positive(entry.probes, 'declaration was never exercised');
    assert.equal(entry.divergences, 0);
    assert.equal(entry.refusals, 0);
    assert.equal(entry.behaviorallyIndistinguishable, true);
    probes += entry.probes;
  }
  assert.equal(identities.size, 0);
  assert.equal(probes, summary.probesEvaluated, 'inconsistent probe accounting');
  return report;
}
