import { execFileSync } from 'node:child_process';
import { lstat, mkdir, mkdtemp, readFile, readdir, realpath, writeFile } from 'node:fs/promises';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import assert from 'node:assert/strict';
import { assertMatrix, assertPassingReport } from './tjsv-evidence.mjs';
import { prepareCorpus, assertCorpusReport } from './tjsv-corpus.mjs';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');

function git(root, args) {
  return execFileSync('git', ['-C', root, ...args], { encoding: 'utf8', timeout: 30_000 }).trim();
}

// Bind the receipt to the committed input/tool-policy closure, not unrelated WIP.
export function assertSourceClean(root) {
  const paths = ['contracts', 'scripts/tjsv-check.mjs', 'scripts/tjsv-evidence.mjs',
    'scripts/tjsv-corpus.mjs', 'tests/tjsv-corpus.test.mjs',
    'tests/docs-serving-contract.test.mjs', 'tests/fixtures/docs-serving-v1.schema.json',
    'tests/tjsv-evidence.test.mjs', 'tests/tjsv-compiler.test.mjs',
    '.github/workflows/tjsv-contract-boundaries.yml', 'package.json', 'package-lock.json'];
  assert.equal(git(root, ['status', '--porcelain', '--untracked-files=all', '--', ...paths]), '',
    'TJSV source closure differs from the recorded commit');
}

async function noSymlinks(path) {
  const info = await lstat(path);
  assert(!info.isSymbolicLink(), 'contract/tool inputs must not be symlinks');
  if (info.isDirectory()) {
    for (const entry of await readdir(path)) await noSymlinks(join(path, entry));
  } else {
    assert(info.isFile(), 'contract input must be a regular file');
  }
}

export async function loadPinnedValidator(root = ROOT) {
  const matrix = assertMatrix(JSON.parse(await readFile(join(root, 'contracts/tjsv.matrix.json'), 'utf8')));
  const toolRoot = join(root, 'target/tools/tjsv');
  assert.equal(await realpath(toolRoot), toolRoot, 'tool checkout must not be redirected');
  assert.equal(git(toolRoot, ['rev-parse', 'HEAD']), matrix.validator.revision, 'TJSV revision mismatch');
  assert.equal(git(toolRoot, ['status', '--porcelain', '--untracked-files=no']), '', 'TJSV tracked source is dirty');
  const validator = await import(pathToFileURL(join(toolRoot, 'src/index.mjs')).href);
  assert.equal(typeof validator.runCheck, 'function', 'TJSV check API unavailable');
  assert.equal(typeof validator.writeReport, 'function', 'TJSV report API unavailable');
  return { matrix, validator, toolRoot };
}

export function checkOptions(typespec, authoredSchema, outputDir, toolRoot, instances) {
  return {
    typespec, authoredSchema, outputDir, instances,
    tspBin: join(toolRoot, 'node_modules/.bin/tsp'),
    maxFindings: 1000, maxProbes: 128, probes: true, formatAssertion: true,
    // Existing middleware transports use JSON numbers for int64. A disagreement
    // with an authored lane remains a finding; it is never silently rewritten.
    int64Strategy: 'number', sealObjectSchemas: true,
  };
}

export async function runMatrix(root = ROOT) {
  assertSourceClean(root);
  const sourceCommit = git(root, ['rev-parse', 'HEAD']);
  const { matrix, validator, toolRoot } = await loadPinnedValidator(root);
  const contracts = join(root, 'contracts');
  await noSymlinks(contracts);
  const parent = join(root, 'target/tjsv');
  await mkdir(parent, { recursive: true });
  assert.equal(await realpath(parent), parent, 'evidence destination must not be redirected');
  // Never consume a prior run's witness or receipt as current evidence.
  const outputRoot = await mkdtemp(join(parent, 'run-'));
  const results = [];
  let exitCode = 0;
  for (const lane of matrix.lanes) {
    let result = { id: lane.id, status: 'failed' };
    try {
      const typespec = await realpath(join(root, lane.typespec));
      const authoredSchema = await realpath(join(root, lane.authoredSchema));
      for (const input of [typespec, authoredSchema]) {
        assert(input.startsWith(`${contracts}${sep}`), 'input escaped authored contracts');
      }
      let prepared;
      let instances;
      if (lane.corpus) {
        const manifest = await realpath(join(root, lane.corpus));
        assert(manifest.startsWith(`${contracts}${sep}`), 'corpus escaped authored contracts');
        instances = join(outputRoot, `${lane.id}-instances`);
        prepared = await prepareCorpus(manifest, instances);
        result.corpus = { manifest: lane.corpus, sha256: prepared.digest, cases: prepared.corpus.cases.length };
      }
      const report = await validator.runCheck(checkOptions(typespec, authoredSchema,
        join(outputRoot, lane.id, 'witness'), toolRoot, instances));
      const receipt = join(outputRoot, `${lane.id}.json`);
      await validator.writeReport(receipt, report);
      result = { ...result, id: lane.id, status: report.status, runId: report.runId,
        receipt: relative(outputRoot, receipt), counts: report.counts,
        differential: report.differential?.summary };
      assertPassingReport(report);
      if (prepared) assertCorpusReport(prepared.corpus, report);
      console.log(`${lane.id}: passed (${report.differential.summary.probesEvaluated} probes)`);
    } catch (error) {
      // Keep exercising later lanes, but never turn a stopped/failed lane green.
      exitCode = 2;
      if (result.status === 'passed') result.status = 'invalid_evidence';
      result.error = error.message;
      console.error(`${lane.id}: blocked; inspect the retained receipt or matrix summary`);
    }
    results.push(result);
  }
  let sourceIntegrity = 'verified';
  try {
    assert.equal(git(root, ['rev-parse', 'HEAD']), sourceCommit, 'source HEAD changed during validation');
    assertSourceClean(root);
  } catch { sourceIntegrity = 'changed'; exitCode = 2; }
  await writeFile(join(outputRoot, 'matrix-summary.json'), `${JSON.stringify({
    schema: 'ores.middleware.tjsv-execution/v1', sourceCommit, sourceIntegrity,
    validator: matrix.validator, status: exitCode === 0 ? 'passed' : 'blocked', results,
  }, null, 2)}\n`, { flag: 'wx' });
  return exitCode;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 2) {
    console.error('This fixed-policy gate accepts no command-line options.');
    process.exitCode = 3;
  } else {
    try { process.exitCode = await runMatrix(); }
    catch { console.error('TJSV setup failed; no contract evidence was produced.'); process.exitCode = 3; }
  }
}
