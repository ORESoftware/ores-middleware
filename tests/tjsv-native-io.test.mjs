import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, mkdir, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

const script = new URL('../scripts/tjsv-native-conformance.mjs', import.meta.url).href;
const fixture = {
  schema: 'ores.generated-runtime-conformance/v1', authority: 'independent-fixture-corpus', model: 'IdempotencyRecord',
  wireFields: ['id'], requiredFields: ['id'], optionalFields: [], statuses: ['pending'],
  cases: [{ id: 'valid', expect: 'accept', value: { id: 'one' } }, { id: 'invalid', expect: 'reject', value: null }],
};
const invoke = (root, task = 'prepareCorpus') => spawnSync(process.execPath,
  ['--input-type=module', '-e', `import { ${task} } from ${JSON.stringify(script)}; await ${task}();`],
  { cwd: root, encoding: 'utf8', timeout: 5000, maxBuffer: 65536 });
async function workspace(t) {
  const root = await mkdtemp(path.join(os.tmpdir(), 'ores-tjsv-io-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  await mkdir(path.join(root, 'fixtures'));
  await writeFile(path.join(root, 'fixtures/generated-runtime-conformance.json'), JSON.stringify(fixture));
  return root;
}
async function assertTombstone(root) {
  const report = JSON.parse(await readFile(path.join(root, 'target/tjsv-native/runtime-report.json')));
  assert.equal(report.status, 'failed');
}
test('prepares exact independent corpus without changing the source', async t => {
  const root = await workspace(t);
  const source = await readFile(path.join(root, 'fixtures/generated-runtime-conformance.json'));
  const result = invoke(root);
  assert.equal(result.status, 0, result.stderr);
  for (const item of fixture.cases) {
    const name = `target/tjsv-native/instances/IdempotencyRecord/${item.expect === 'accept' ? 'valid' : 'invalid'}/${item.id}.json`;
    assert.deepEqual(JSON.parse(await readFile(path.join(root, name))), item.value);
  }
  assert.deepEqual(await readFile(path.join(root, 'fixtures/generated-runtime-conformance.json')), source);
  await assertTombstone(root);
});
test('refuses a reused corpus and invalidates an old passing report', async t => {
  const root = await workspace(t);
  assert.equal(invoke(root).status, 0);
  await writeFile(path.join(root, 'target/tjsv-native/runtime-report.json'), '{"status":"passed"}');
  assert.notEqual(invoke(root).status, 0);
  await assertTombstone(root);
});
test('refuses symlinked corpus files', async t => {
  const root = await workspace(t);
  const name = path.join(root, 'fixtures/generated-runtime-conformance.json');
  const backing = path.join(root, 'backing.json');
  await writeFile(backing, await readFile(name));
  await rm(name);
  await symlink(backing, name);
  assert.notEqual(invoke(root).status, 0);
  await assertTombstone(root);
});
test('refuses symlinked corpus parent directories', async t => {
  const root = await workspace(t);
  const backing = path.join(root, 'other-fixtures');
  await mkdir(backing);
  await writeFile(path.join(backing, 'generated-runtime-conformance.json'), JSON.stringify(fixture));
  await rm(path.join(root, 'fixtures'), { recursive: true });
  await symlink(backing, path.join(root, 'fixtures'));
  assert.notEqual(invoke(root).status, 0);
  await assertTombstone(root);
});
test('refuses oversized input before JSON parsing', async t => {
  const root = await workspace(t);
  await writeFile(path.join(root, 'fixtures/generated-runtime-conformance.json'), Buffer.alloc(4 * 1024 * 1024 + 1, 32));
  const result = invoke(root);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /bounded regular file/);
  await assertTombstone(root);
});
test('refuses malformed input with a non-passing report', async t => {
  const root = await workspace(t);
  await writeFile(path.join(root, 'fixtures/generated-runtime-conformance.json'), '{bad-json');
  assert.notEqual(invoke(root).status, 0);
  await assertTombstone(root);
});
test('missing TJSV cannot leave stale admitted output', async t => {
  const root = await workspace(t);
  await mkdir(path.join(root, 'target/tjsv-native'), { recursive: true });
  await writeFile(path.join(root, 'target/tjsv-native/runtime-report.json'), '{"status":"passed"}');
  assert.notEqual(invoke(root, 'admitNativeEvidence').status, 0);
  await assertTombstone(root);
});
