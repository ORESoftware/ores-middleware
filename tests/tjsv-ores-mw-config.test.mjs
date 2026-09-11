import assert from 'node:assert/strict';
import test from 'node:test';
import { resolve } from 'node:path';

import {
  DECLARATIONS,
  TJSV_REV,
  assertSuccessfulReceipt,
  childEnvironment,
  snapshotInputs,
} from '../scripts/check-tjsv-ores-mw.mjs';

const root = resolve(import.meta.dirname, '..');
const CORPUS_INSTANCES = 12;
const SNAPSHOT_FILES = 14;

function successfulReport(corpusInstances) {
  const rows = DECLARATIONS.map((name, index) => ({
    authored: name,
    generated: name,
    typespec: `Ores.Middleware.Config.${name}`,
    probes: index + 4,
    divergences: 0,
    refusals: 0,
    behaviorallyIndistinguishable: true,
  }));
  const probes = rows.reduce((sum, row) => sum + row.probes, 0);
  return {
    schema: 'ores.typespec-json-schema-validator.report/v1',
    runId: 'a'.repeat(64),
    status: 'passed',
    zeroUnexplainedFindings: true,
    findings: [],
    counts: {
      findings: 0,
      differentialFindings: 0,
      emittedFindings: 0,
      structuralFindings: 0,
      typespecOutOfScopeDeclarations: 0,
      findingsTruncated: false,
      authoredDeclarations: DECLARATIONS.length,
      generatedDeclarations: DECLARATIONS.length,
      typespecDeclarations: DECLARATIONS.length,
    },
    coverage: {
      differentialInstanceValidation: true,
      directDeclarationInventory: true,
      sourceMutationCheck: true,
      typespecGeneratedJsonSchemaComparison: true,
      outOfScopeTypeSpecDeclarations: [],
    },
    differential: {
      summary: {
        comparedDeclarations: DECLARATIONS.length,
        probesEvaluated: probes,
        agreements: probes,
        divergences: 0,
        refusals: 0,
        behaviorallyIndistinguishableDeclarations: DECLARATIONS.length,
        corpusInstances,
      },
      declarations: rows,
    },
  };
}

test('pins a full immutable TJSV revision', () => {
  assert.match(TJSV_REV, /^[0-9a-f]{40}$/);
});

test('fixture snapshot covers all four declarations with valid and invalid lanes', () => {
  const value = snapshotInputs(root);
  assert.equal(value.corpusInstances, CORPUS_INSTANCES);
  assert.equal(value.files, SNAPSHOT_FILES);
  assert.match(value.digest, /^[0-9a-f]{64}$/);
});

test('admission policy requires complete exact declaration/probe/corpus evidence', () => {
  assert.doesNotThrow(() => assertSuccessfulReceipt(
    successfulReport(CORPUS_INSTANCES),
    CORPUS_INSTANCES,
  ));
  for (const mutate of [
    (report) => { report.status = 'stopped_for_evaluation'; },
    (report) => { report.counts.structuralFindings = 1; },
    (report) => { report.differential.summary.corpusInstances = CORPUS_INSTANCES - 1; },
    (report) => { report.differential.declarations[0].typespec = 'Wrong.Namespace'; },
    (report) => { report.differential.declarations[0].refusals = 1; },
  ]) {
    const report = successfulReport(CORPUS_INSTANCES);
    mutate(report);
    assert.throws(() => assertSuccessfulReceipt(report, CORPUS_INSTANCES));
  }
});

test('child environment strips validator overrides and credentials', () => {
  assert.deepEqual(
    childEnvironment({ PATH: '/bin', HOME: '/tmp/home', GH_TOKEN: 'secret', TJSV_FLAGS: '--unsafe' }),
    { PATH: '/bin', HOME: '/tmp/home' },
  );
});
