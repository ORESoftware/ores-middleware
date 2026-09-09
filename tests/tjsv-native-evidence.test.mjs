import assert from 'node:assert/strict';
import test from 'node:test';
import { buildNativeEvidence, cells, digest, fixturePath, languages, lanes, parseFixture, sourcePaths } from '../scripts/lib/tjsv-native-evidence.mjs';

const json = value => Buffer.from(JSON.stringify(value));
function scenario() {
  // Synthetic unit fixture; this is not evidence of a native runtime execution.
  const fixture = {
    schema: 'ores.generated-runtime-conformance/v1', authority: 'independent-fixture-corpus', model: 'IdempotencyRecord',
    wireFields: ['id'], requiredFields: ['id'], optionalFields: [], statuses: ['pending'],
    cases: [{ id: 'valid', expect: 'accept', value: { id: 'one' } }, { id: 'invalid', expect: 'reject', value: null }],
  };
  const files = new Map(sourcePaths.map(name => [name, Buffer.from(name)]));
  files.set(fixturePath, json(fixture));
  const harnessPaths = ['tests/generated-runtime/example.mjs'];
  files.set(harnessPaths[0], Buffer.from('synthetic harness'));
  const witnesses = cells.map(cell => {
    files.set(cell.artifact, Buffer.from(`synthetic ${cell.id}`));
    files.set(cell.result, json({
      schema: 'ores.generated-runtime-witness/v1', authority: cell.authority, language: cell.language,
      model: fixture.model, wireFields: fixture.wireFields, requiredFields: fixture.requiredFields,
      optionalFields: fixture.optionalFields, statuses: fixture.statuses, statusAcceptance: { pending: true, __unknown__: false },
      cases: [{ id: 'valid', accepted: true, normalized: { id: 'one' } }, { id: 'invalid', accepted: false, normalized: null }],
    }));
    return { authority: cell.authority, language: cell.language, state: 'executed', generatedArtifact: cell.artifact,
      generatedArtifactSha256: digest(files.get(cell.artifact)), result: cell.result, resultSha256: digest(files.get(cell.result)) };
  });
  const commit = 'a'.repeat(40);
  return {
    binding: { contractIrId: 'b'.repeat(64), inputDigest: 'c'.repeat(64) }, commit, files, harnessPaths,
    toolchains: Object.fromEntries(languages.map(value => [value, 'synthetic-unit-test-toolchain'])),
    receipt: {
      schema: 'ores.generated-runtime-convergence-report/v1', repository: 'ORESoftware/ores-middleware', commit,
      status: 'passed', zeroUnexplainedFindings: true, discrepancies: [], authorities: [...lanes], languages: [...languages],
      fixture: { path: fixturePath, schema: fixture.schema, authority: fixture.authority, cases: 2, sha256: digest(files.get(fixturePath)) },
      sourceDigests: Object.fromEntries(sourcePaths.map(name => [name, digest(files.get(name))])),
      harnessDigests: { [harnessPaths[0]]: digest(files.get(harnessPaths[0])) }, witnesses,
      checks: cells.map(cell => ({ id: `${cell.id}/runtime`, state: 'executed', exitCode: 0 })),
    },
  };
}
function changeWitness(input, change) {
  const item = input.receipt.witnesses[0];
  const witness = JSON.parse(input.files.get(item.result));
  change(witness);
  input.files.set(item.result, json(witness));
  item.resultSha256 = digest(input.files.get(item.result));
}
test('translates all twelve actual witness verdict sets deterministically', () => {
  const input = scenario();
  const result = buildNativeEvidence(input);
  assert.equal(result.evidence.adapters.length, 12);
  assert.equal(result.requiredAdapters.length, 12);
  assert.equal(result.evidence.adapters[0].results[1].verdict, 'rejected');
  assert.deepEqual(result, buildNativeEvidence(input));
});
const mutations = {
  'duplicate witness case even with matching digest': input => changeWitness(input, value => value.cases.push(value.cases[0])),
  'missing witness case': input => changeWitness(input, value => value.cases.pop()),
  'extra witness case': input => changeWitness(input, value => value.cases.push({ id: 'extra', accepted: true, normalized: {} })),
  'false acceptance': input => changeWitness(input, value => { value.cases[1].accepted = true; }),
  'non-boolean acceptance': input => changeWitness(input, value => { value.cases[0].accepted = 1; }),
  'changed roundtrip': input => changeWitness(input, value => { value.cases[0].normalized.id = 'other'; }),
  'incorrect language': input => changeWitness(input, value => { value.language = 'bun'; }),
  'duplicate field': input => changeWitness(input, value => value.wireFields.push('id')),
  'incorrect status behavior': input => changeWitness(input, value => { value.statusAcceptance.__unknown__ = true; }),
  'missing cell': input => input.receipt.witnesses.pop(),
  'duplicate cell': input => { input.receipt.witnesses[1] = input.receipt.witnesses[0]; },
  'skipped cell': input => { input.receipt.witnesses[0].state = 'skipped'; },
  'partial receipt': input => { input.receipt.status = 'partial'; },
  'stale commit': input => { input.receipt.commit = 'd'.repeat(40); },
  'absent expected commit': input => { input.commit = null; },
  'stale authority': input => input.files.set(sourcePaths[0], Buffer.from('changed source')),
  'stale corpus': input => input.files.set(fixturePath, Buffer.concat([input.files.get(fixturePath), Buffer.from('\n')])),
  'stale generated artifact': input => input.files.set(cells[0].artifact, Buffer.from('changed artifact')),
  'stale witness digest': input => { input.receipt.witnesses[0].resultSha256 = '0'.repeat(64); },
  'missing harness digest': input => { input.receipt.harnessDigests = {}; },
  'changed harness': input => input.files.set(input.harnessPaths[0], Buffer.from('changed harness')),
  'missing current file': input => input.files.delete(cells[0].artifact),
  'unsafe result path': input => { input.receipt.witnesses[0].result = '../../outside.json'; },
  'missing execution record': input => input.receipt.checks.pop(),
  'duplicate execution record': input => input.receipt.checks.push(input.receipt.checks[0]),
  'failed native execution': input => { input.receipt.checks[0].exitCode = 1; },
  'missing measured toolchain': input => { delete input.toolchains.rust; },
  'unexplained native finding': input => input.receipt.discrepancies.push({ kind: 'drift' }),
  'reduced required runtime inventory': input => input.receipt.languages.pop(),
  'invalid contract IR digest': input => { input.binding.contractIrId = 'unverified'; },
};
for (const [name, mutate] of Object.entries(mutations)) test(`rejects ${name}`, () => {
  const input = scenario();
  mutate(input);
  assert.throws(() => buildNativeEvidence(input));
});
for (const [name, mutate] of Object.entries({
  duplicate: value => value.cases.push(value.cases[0]),
  unsafe: value => { value.cases[0].id = '../escape'; },
  expectation: value => { value.cases[0].expect = 'skip'; },
  empty: value => { value.cases = []; },
  missingValue: value => { delete value.cases[0].value; },
  noNegative: value => { value.cases[1].expect = 'accept'; },
})) test(`rejects ${name} corpus`, () => {
  const value = JSON.parse(scenario().files.get(fixturePath));
  mutate(value);
  assert.throws(() => parseFixture(json(value)));
});
