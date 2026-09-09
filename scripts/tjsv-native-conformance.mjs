import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { constants } from 'node:fs';
import { lstat, mkdir, open, readdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { buildNativeEvidence, cells, digest, fixturePath, languages, parseFixture, sourcePaths } from './lib/tjsv-native-evidence.mjs';

const root = process.cwd();
const output = 'target/tjsv-native';
const pin = '4473504c4c9d2831d825919f70c03994d8ce01d2';
const tool = 'tmp/tjsv-tool';
const nativeReceipt = 'target/generated-runtime-convergence/receipt.json';
const command = (file, args) => execFileSync(file, args, {
  cwd: root, encoding: 'utf8', timeout: 30000, maxBuffer: 65536, stdio: ['ignore', 'pipe', 'pipe'],
}).trim();
const writeJson = async (name, value) => {
  await mkdir(path.dirname(path.join(root, name)), { recursive: true });
  await writeFile(path.join(root, name), `${JSON.stringify(value, null, 2)}\n`);
};

// All input paths are fixed policy paths or independently enumerated harnesses.
// Refuse symlinks and boundedly read regular files, not receipt-selected paths.
async function readBytes(name) {
  assert(!path.isAbsolute(name) && !name.split('/').some(part => ['', '.', '..'].includes(part)), 'unsafe input path');
  let current = root;
  for (const part of name.split('/')) {
    current = path.join(current, part);
    assert(!(await lstat(current)).isSymbolicLink(), 'symlink input is not admissible');
  }
  const handle = await open(current, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    const info = await handle.stat();
    assert(info.isFile() && info.size <= 4 * 1024 * 1024, 'input must be a bounded regular file');
    const bytes = Buffer.alloc(info.size + 1);
    let length = 0;
    while (length < bytes.length) {
      const read = await handle.read(bytes, length, bytes.length - length, null);
      if (read.bytesRead === 0) break;
      length += read.bytesRead;
    }
    assert.equal(length, info.size, 'input changed size while being read');
    return bytes.subarray(0, length);
  } finally { await handle.close(); }
}
async function harnessInventory(directory = 'tests/generated-runtime') {
  assert(!(await lstat(path.join(root, directory))).isSymbolicLink(), 'symlink harness directory');
  const files = [];
  for (const entry of await readdir(path.join(root, directory), { withFileTypes: true })) {
    const name = `${directory}/${entry.name}`;
    assert(!entry.isSymbolicLink(), 'symlink harness entry');
    if (entry.isDirectory()) files.push(...await harnessInventory(name));
    else { assert(entry.isFile(), 'non-regular harness entry'); files.push(name); }
  }
  return files.sort();
}

/** Fixed-path task, no separate command-line option parser. */
export async function prepareCorpus() {
  // mkdir without recursive fails on reuse: old instances cannot survive a rerun.
  await mkdir(path.join(root, output), { recursive: true });
  await writeJson(`${output}/runtime-report.json`, { status: 'failed', stage: 'not-yet-admitted' });
  await mkdir(path.join(root, output, 'instances'));
  const fixture = parseFixture(await readBytes(fixturePath));
  for (const item of fixture.cases) {
    await writeJson(`${output}/instances/${fixture.model}/${item.expect === 'accept' ? 'valid' : 'invalid'}/${item.id}.json`, item.value);
  }
}

/** Admit actual native evidence only after fresh TJSV source verification. */
export async function admitNativeEvidence() {
  await writeJson(`${output}/runtime-report.json`, { status: 'failed', stage: 'verification-started' });
  assert.equal(command('git', ['-C', tool, 'rev-parse', 'HEAD']), pin, 'unexpected TJSV revision');
  const commit = command('git', ['rev-parse', 'HEAD']);
  if (process.env.GITHUB_SHA) assert.equal(commit, process.env.GITHUB_SHA, 'checkout is not the workflow source');
  const api = await import(pathToFileURL(path.join(root, tool, 'src/runtime-conformance/index.mjs')).href);
  const inputs = {
    contractIr: JSON.parse(await readBytes(`${output}/contract-ir.json`)),
    parityReport: JSON.parse(await readBytes(`${output}/parity-report.json`)),
    typespec: path.join(root, sourcePaths[0]), authoredSchema: path.join(root, sourcePaths[1]),
  };
  const binding = await api.createRuntimeEvidenceBindingAgainstCurrentInputs(inputs);
  const harnessPaths = await harnessInventory();
  const filePaths = [...sourcePaths, ...harnessPaths, ...cells.flatMap(cell => [cell.artifact, cell.result])];
  const files = new Map(await Promise.all(filePaths.map(async name => [name, await readBytes(name)])));
  const nativeBytes = await readBytes(nativeReceipt);
  const toolchains = {
    typescript: process.version,
    rust: command('rustc', ['--version']),
    golang: command('go', ['version']),
    gleam: command('gleam', ['--version']),
    elixir: command('elixir', ['--version']),
    erlang: command('erl', ['-noshell', '-eval', 'io:format("~s", [erlang:system_info(otp_release)]), halt().']),
  };
  assert.equal(Object.keys(toolchains).length, languages.length);
  const translated = buildNativeEvidence({ binding, receipt: JSON.parse(nativeBytes), files, harnessPaths, commit, toolchains });
  const options = { ...inputs, ...translated, expectedCorpusDigest: digest(files.get(fixturePath)) };
  const report = await api.verifyRuntimeEvidenceAgainstCurrentInputs(options);
  assert.equal(report.status, 'passed', 'TJSV did not admit native evidence');
  assert.equal(report.contractIrVerified, true);
  assert.equal(report.zeroUnexplainedFindings, true);
  await writeJson(`${output}/runtime-evidence.json`, translated.evidence);
  await writeJson(`${output}/provenance.json`, {
    schema: 'ores.middleware.tjsv-native-provenance/v1', commit, validatorCommit: pin,
    nativeReceiptSha256: digest(nativeBytes),
    files: Object.fromEntries([...files].map(([name, bytes]) => [name, digest(bytes)])),
  });

  // Real TJSV rejection checks run only after the unchanged baseline passes.
  const regressions = [];
  async function reject(name, mutated) {
    let rejected = false;
    try { rejected = (await api.verifyRuntimeEvidenceAgainstCurrentInputs(mutated)).status !== 'passed'; }
    catch { rejected = true; }
    assert(rejected, `TJSV accepted ${name}`);
    regressions.push({ name, rejected: true });
  }
  for (const [name, change] of [
    ['duplicate-case', value => value.adapters[0].results.push(value.adapters[0].results[0])],
    ['missing-runtime', value => value.adapters.pop()],
    ['skipped-runtime', value => { value.adapters[0].status = 'skipped'; }],
    ['incorrect-verdict', value => { value.adapters[0].results[0].verdict = 'error'; }],
    ['stale-corpus-digest', value => { value.corpusDigest = '0'.repeat(64); }],
  ]) {
    const evidence = structuredClone(translated.evidence);
    change(evidence);
    await reject(name, { ...options, evidence });
  }
  await reject('tampered-contract-ir', { ...options, contractIr: {} });
  const changed = JSON.parse(files.get(sourcePaths[1]));
  changed.$defs.IdempotencyRecord.properties.id.type = 'integer';
  await writeJson(`${output}/changed.schema.json`, changed);
  await reject('changed-current-authority', { ...options, authoredSchema: path.join(root, output, 'changed.schema.json') });
  await writeJson(`${output}/admission-regressions.json`, { status: 'passed', regressions });
  // The final pass is written last; any earlier failure leaves a tombstone.
  await writeJson(`${output}/runtime-report.json`, report);
  console.log(`TJSV admitted ${report.summary.passedAdapters} native adapters; ${regressions.length} rejection regressions passed`);
}
