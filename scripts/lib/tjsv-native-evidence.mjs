import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';

export const lanes = Object.freeze(['typespec', 'json-schema-openapi']);
export const languages = Object.freeze(['typescript', 'rust', 'golang', 'gleam', 'elixir', 'erlang']);
export const declarationId = 'Ores.Middleware.Persistence.IdempotencyRecord';
export const fixturePath = 'fixtures/generated-runtime-conformance.json';
export const sourcePaths = Object.freeze([
  'contracts/persistence/idempotency-record.tsp',
  'contracts/persistence/idempotency-record.schema.json',
  fixturePath,
  'scripts/generated_runtime_matrix.mjs',
]);
const extensions = { typescript: 'mjs', rust: 'rs', golang: 'go', gleam: 'gleam', elixir: 'ex', erlang: 'erl' };
export const cells = Object.freeze(lanes.flatMap(authority => languages.map(language => Object.freeze({
  id: `${authority}/${language}`, authority, language,
  artifact: `target/schema-convergence/${authority}/${language}/idempotency_record.${extensions[language]}`,
  result: `target/generated-runtime-convergence/results/${authority}/${language}.json`,
}))));
export const digest = bytes => createHash('sha256').update(bytes).digest('hex');

function unique(items, label) {
  assert(Array.isArray(items), `${label}: expected array`);
  assert(items.every(value => typeof value === 'string' && value.length > 0), `${label}: invalid identifier`);
  assert.equal(new Set(items).size, items.length, `${label}: duplicate identifier`);
  return [...items].sort();
}
function exact(actual, expected, label) {
  assert.deepEqual(unique(actual, label), unique(expected, label), `${label}: incomplete or unexpected set`);
}
function sha(value, label) {
  assert(typeof value === 'string' && /^[a-f0-9]{64}$/.test(value), `${label}: invalid SHA-256`);
}

export function parseFixture(bytes) {
  const fixture = JSON.parse(bytes.toString('utf8'));
  assert.equal(fixture.schema, 'ores.generated-runtime-conformance/v1');
  assert.equal(fixture.authority, 'independent-fixture-corpus');
  assert.equal(fixture.model, 'IdempotencyRecord');
  assert(Array.isArray(fixture.cases) && fixture.cases.length > 0 && fixture.cases.length <= 10000);
  unique(fixture.cases.map(item => item.id), 'fixture cases');
  for (const item of fixture.cases) {
    assert(/^[a-zA-Z0-9_-]+$/.test(item.id), 'unsafe corpus case identifier');
    assert(['accept', 'reject'].includes(item.expect), 'invalid corpus expectation');
    assert(Object.hasOwn(item, 'value'), 'missing corpus value');
  }
  assert(fixture.cases.some(item => item.expect === 'accept'), 'missing positive corpus');
  assert(fixture.cases.some(item => item.expect === 'reject'), 'missing negative corpus');
  for (const key of ['wireFields', 'requiredFields', 'optionalFields', 'statuses']) unique(fixture[key], key);
  exact([...fixture.requiredFields, ...fixture.optionalFields], fixture.wireFields, 'field partition');
  return fixture;
}

/** Convert executed witnesses, never expected verdicts, into TJSV evidence. */
export function buildNativeEvidence({ binding, receipt, files, harnessPaths, commit, toolchains }) {
  assert(/^[a-f0-9]{40}$/.test(commit ?? ''), 'expected immutable source commit');
  sha(binding.contractIrId, 'contract IR');
  sha(binding.inputDigest, 'input digest');
  assert(files instanceof Map, 'expected current file bytes');
  const checked = (name, expectedDigest) => {
    sha(expectedDigest, name);
    const bytes = files.get(name);
    assert(Buffer.isBuffer(bytes), `missing current bytes: ${name}`);
    assert.equal(digest(bytes), expectedDigest, `stale or tampered bytes: ${name}`);
    return bytes;
  };
  const fixture = parseFixture(files.get(fixturePath));
  assert.equal(receipt.schema, 'ores.generated-runtime-convergence-report/v1');
  assert.equal(receipt.repository, 'ORESoftware/ores-middleware');
  assert.equal(receipt.commit, commit, 'stale source commit');
  assert.equal(receipt.status, 'passed', 'native execution did not pass');
  assert.equal(receipt.zeroUnexplainedFindings, true);
  assert.deepEqual(receipt.discrepancies, []);
  exact(receipt.authorities, lanes, 'authority lanes');
  exact(receipt.languages, languages, 'native languages');
  assert.equal(receipt.fixture.path, fixturePath);
  assert.equal(receipt.fixture.schema, fixture.schema);
  assert.equal(receipt.fixture.authority, fixture.authority);
  assert.equal(receipt.fixture.cases, fixture.cases.length);
  checked(fixturePath, receipt.fixture.sha256);
  exact(Object.keys(receipt.sourceDigests), sourcePaths, 'source digest set');
  for (const name of sourcePaths) checked(name, receipt.sourceDigests[name]);
  assert(harnessPaths.length > 0, 'missing current harness inventory');
  exact(Object.keys(receipt.harnessDigests), harnessPaths, 'harness inventory');
  for (const name of harnessPaths) checked(name, receipt.harnessDigests[name]);
  unique(receipt.checks.map(item => item.id), 'execution checks');
  for (const check of receipt.checks) {
    assert.equal(check.state, 'executed', 'unexecuted native check');
    assert.equal(check.exitCode, 0, 'failed native command');
  }
  exact(receipt.witnesses.map(item => `${item.authority}/${item.language}`), cells.map(item => item.id), 'native cells');
  const expectedCases = fixture.cases.map(item => ({
    id: item.id, declaration: declarationId, expectation: item.expect === 'accept' ? 'accepted' : 'rejected',
  }));
  const adapters = cells.map(cell => {
    const item = receipt.witnesses.find(value => `${value.authority}/${value.language}` === cell.id);
    assert.equal(item.state, 'executed', `unexecuted cell: ${cell.id}`);
    assert(receipt.checks.some(check => check.id === `${cell.id}/runtime`), `missing runtime command: ${cell.id}`);
    assert.equal(item.generatedArtifact, cell.artifact, 'unexpected generated artifact path');
    assert.equal(item.result, cell.result, 'unexpected result path');
    checked(cell.artifact, item.generatedArtifactSha256);
    const witness = JSON.parse(checked(cell.result, item.resultSha256).toString('utf8'));
    assert.equal(witness.schema, 'ores.generated-runtime-witness/v1');
    assert.equal(witness.authority, cell.authority);
    assert.equal(witness.language, cell.language);
    assert.equal(witness.model, fixture.model);
    for (const key of ['wireFields', 'requiredFields', 'optionalFields', 'statuses']) exact(witness[key], fixture[key], key);
    assert.deepEqual(witness.statusAcceptance, Object.fromEntries([...fixture.statuses.map(value => [value, true]), ['__unknown__', false]]));
    exact(witness.cases.map(value => value.id), fixture.cases.map(value => value.id), 'witness cases');
    const results = witness.cases.map(value => {
      assert.equal(typeof value.accepted, 'boolean', 'verdict must be boolean');
      const expected = fixture.cases.find(entry => entry.id === value.id);
      assert.equal(value.accepted, expected.expect === 'accept', 'runtime verdict mismatch');
      assert.deepEqual(value.normalized, value.accepted ? expected.value : null, 'runtime roundtrip mismatch');
      return { caseId: value.id, declaration: declarationId, verdict: value.accepted ? 'accepted' : 'rejected' };
    });
    assert(typeof toolchains[cell.language] === 'string' && toolchains[cell.language].trim().length > 0, 'missing measured toolchain');
    return {
      id: cell.id, language: cell.language,
      runtime: cell.language === 'typescript' ? 'nodejs' : cell.language,
      validator: 'generated-idempotency-record', toolchain: toolchains[cell.language], status: 'passed', results,
    };
  });
  return {
    evidence: { schema: 'ores.typespec-json-schema-validator.runtime-evidence/v1', ...binding, corpusDigest: digest(files.get(fixturePath)), adapters },
    expectedCases,
    requiredAdapters: cells.map(cell => ({ id: cell.id, language: cell.language, validator: 'generated-idempotency-record' })),
  };
}
