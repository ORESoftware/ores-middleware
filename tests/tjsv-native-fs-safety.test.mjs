import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { link, lstat, mkdtemp, mkdir, readFile, readdir, rm, symlink, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

const script = new URL('../scripts/tjsv-native-conformance.mjs', import.meta.url).href;
const fixture = {
  schema: 'ores.generated-runtime-conformance/v1', authority: 'independent-fixture-corpus', model: 'IdempotencyRecord',
  wireFields: ['id'], requiredFields: ['id'], optionalFields: [], statuses: ['pending'],
  cases: [{ id: 'valid', expect: 'accept', value: { id: 'one' } }, { id: 'invalid', expect: 'reject', value: null }],
};
const sentinel = '{"status":"passed","owner":"outside-workspace"}\n';
const invoke = (root, task = 'prepareCorpus') => spawnSync(process.execPath,
  ['--input-type=module', '-e', `import { ${task} } from ${JSON.stringify(script)}; await ${task}();`],
  { cwd: root, encoding: 'utf8', timeout: 3000, maxBuffer: 65536 });
function rejectedPromptly(result, pattern) {
  assert.equal(result.error, undefined, `task must reject without timeout: ${result.error}`);
  assert.equal(result.signal, null, 'task must not need termination');
  assert.notEqual(result.status, 0, 'unsafe filesystem entry was accepted');
  assert.match(result.stderr, pattern);
}
async function workspace(t) {
  const temporary = await mkdtemp(path.join(os.tmpdir(), 'ores-tjsv-fs-safety-'));
  t.after(() => rm(temporary, { recursive: true, force: true }));
  const root = path.join(temporary, 'work');
  const outside = path.join(temporary, 'outside');
  await mkdir(path.join(root, 'fixtures'), { recursive: true });
  await mkdir(outside);
  await writeFile(path.join(root, 'fixtures/generated-runtime-conformance.json'), JSON.stringify(fixture));
  return { root, outside };
}
for (const task of ['prepareCorpus', 'admitNativeEvidence']) {
  for (const directory of ['target', 'target/tjsv-native']) {
    test(`${task} refuses linked output parent ${directory} without external writes`, async t => {
      const { root, outside } = await workspace(t);
      const externalOutput = directory === 'target' ? path.join(outside, 'tjsv-native') : outside;
      await mkdir(externalOutput, { recursive: true });
      const backing = path.join(externalOutput, 'runtime-report.json');
      await writeFile(backing, sentinel);
      const destination = path.join(root, directory);
      await mkdir(path.dirname(destination), { recursive: true });
      await symlink(outside, destination);
      const result = invoke(root, task);
      assert.equal(await readFile(backing, 'utf8'), sentinel, 'outside report must remain unchanged');
      assert.deepEqual(await readdir(externalOutput), ['runtime-report.json']);
      assert((await lstat(destination)).isSymbolicLink());
      rejectedPromptly(result, /unsafe output directory/);
    });
  }
  for (const kind of ['symlink', 'hardlink', 'dangling-symlink']) {
    test(`${task} refuses ${kind} report without changing its target`, async t => {
      const { root, outside } = await workspace(t);
      const output = path.join(root, 'target/tjsv-native');
      await mkdir(output, { recursive: true });
      const backing = path.join(outside, 'report.json');
      const destination = path.join(output, 'runtime-report.json');
      if (kind !== 'dangling-symlink') await writeFile(backing, sentinel);
      if (kind === 'hardlink') await link(backing, destination);
      else await symlink(backing, destination);
      const result = invoke(root, task);
      if (kind === 'dangling-symlink') await assert.rejects(lstat(backing), { code: 'ENOENT' });
      else assert.equal(await readFile(backing, 'utf8'), sentinel, 'link target must remain unchanged');
      assert.deepEqual(await readdir(output), ['runtime-report.json'], 'no temporary writes left behind');
      rejectedPromptly(result, /unsafe output file/);
    });
  }
}
test('FIFO corpus input is rejected without waiting for a writer', { skip: process.platform === 'win32' }, async t => {
  const { root } = await workspace(t);
  const name = path.join(root, 'fixtures/generated-runtime-conformance.json');
  await rm(name);
  const fifo = spawnSync('mkfifo', [name], { encoding: 'utf8' });
  assert.equal(fifo.status, 0, fifo.stderr);
  rejectedPromptly(invoke(root), /bounded regular file/);
  const report = JSON.parse(await readFile(path.join(root, 'target/tjsv-native/runtime-report.json')));
  assert.equal(report.status, 'failed');
});
for (const task of ['prepareCorpus', 'admitNativeEvidence']) {
  test(`${task} rejects FIFO output without waiting for a reader`, { skip: process.platform === 'win32' }, async t => {
    const { root } = await workspace(t);
    const output = path.join(root, 'target/tjsv-native');
    await mkdir(output, { recursive: true });
    const destination = path.join(output, 'runtime-report.json');
    const fifo = spawnSync('mkfifo', [destination], { encoding: 'utf8' });
    assert.equal(fifo.status, 0, fifo.stderr);
    rejectedPromptly(invoke(root, task), /unsafe output file/);
    assert((await lstat(destination)).isFIFO());
    assert.deepEqual(await readdir(output), ['runtime-report.json']);
  });
}
test('regular corpus preparation writes complete JSON with no temporary files', async t => {
  const { root } = await workspace(t);
  const result = invoke(root);
  assert.equal(result.status, 0, result.stderr);
  const output = path.join(root, 'target/tjsv-native');
  assert.deepEqual((await readdir(output)).sort(), ['instances', 'runtime-report.json']);
  const report = JSON.parse(await readFile(path.join(output, 'runtime-report.json')));
  assert.equal(report.status, 'failed');
  for (const item of fixture.cases) {
    const directory = path.join(output, 'instances/IdempotencyRecord', item.expect === 'accept' ? 'valid' : 'invalid');
    assert.deepEqual(await readdir(directory), [`${item.id}.json`]);
    assert.deepEqual(JSON.parse(await readFile(path.join(directory, `${item.id}.json`))), item.value);
  }
});
