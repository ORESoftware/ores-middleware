#!/usr/bin/env node
/** Additive docs-serving DATA-contract gate; not an adapter/runtime certificate. */
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, writeFileSync,
} from 'node:fs';
import { dirname, join, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

export const TJSV_REV = '6bb5b7c1ee41c8b43741e50a264c33a1165549c4';
export const DECLARATIONS = Object.freeze([
  'DocsAction', 'DocsDecision', 'DocsRepresentation', 'DocsRequest',
]);
const REPORT_SCHEMA = 'ores.typespec-json-schema-validator.report/v1';
const CORPUS = 'contracts/tjsv-instances/docs-serving';
const MAX_FILE_BYTES = 64 * 1024;
const MAX_REPORT_BYTES = 4 * 1024 * 1024;
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const object = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
const positive = (value) => Number.isSafeInteger(value) && value > 0;
const json = (value) => `${JSON.stringify(value, null, 2)}\n`;

/** Ignore inherited validator overrides, Node injection options and credentials. */
export function childEnvironment(env = process.env) {
  return Object.fromEntries(['PATH', 'HOME', 'TMPDIR', 'TEMP', 'TMP', 'SystemRoot']
    .filter((key) => typeof env[key] === 'string')
    .map((key) => [key, env[key]]));
}

function checkedPath(root, relativePath, kind) {
  const parts = relativePath.split('/');
  assert(parts.every((part) => part && part !== '.' && part !== '..'), 'unsafe-relative-path');
  let path = root;
  for (let i = 0; i < parts.length; i += 1) {
    path = join(path, parts[i]);
    const stat = lstatSync(path);
    assert(!stat.isSymbolicLink(), 'symlink-not-allowed');
    if (i < parts.length - 1 || kind === 'directory') assert(stat.isDirectory(), 'directory-required');
    else assert(stat.isFile() && stat.nlink === 1, 'independent-regular-file-required');
  }
  assert(path.startsWith(`${root}${sep}`), 'path-outside-root');
  return path;
}

function readBounded(path, limit = MAX_FILE_BYTES) {
  const stat = lstatSync(path);
  assert(stat.isFile() && !stat.isSymbolicLink() && stat.nlink === 1, 'independent-regular-file-required');
  assert(stat.size <= limit, 'input-too-large');
  const bytes = readFileSync(path);
  assert(bytes.length <= limit, 'input-too-large');
  return bytes;
}

/** Inventory and hash every fixture: no ignored names, empty lanes or symlinks. */
export function snapshotInputs(root) {
  const corpusPath = checkedPath(root, CORPUS, 'directory');
  assert.deepEqual(readdirSync(corpusPath).sort(), DECLARATIONS, 'unexpected-corpus-declaration');
  const paths = ['contracts/docs-serving.tsp', 'contracts/docs-serving.schema.json'];
  let corpusInstances = 0;
  for (const name of DECLARATIONS) {
    const declarationPath = checkedPath(root, `${CORPUS}/${name}`, 'directory');
    assert.deepEqual(readdirSync(declarationPath).sort(), ['invalid', 'valid'], 'expectation-lanes-required');
    for (const lane of ['invalid', 'valid']) {
      const relativeDirectory = `${CORPUS}/${name}/${lane}`;
      const directory = checkedPath(root, relativeDirectory, 'directory');
      const names = readdirSync(directory).sort();
      assert(names.length > 0, 'empty-expectation-lane');
      for (const filename of names) {
        assert(/^[a-z0-9][a-z0-9-]*\.json$/.test(filename), 'invalid-fixture-filename');
        paths.push(`${relativeDirectory}/${filename}`);
        corpusInstances += 1;
        assert(corpusInstances <= 512, 'too-many-fixtures');
      }
    }
  }
  const hash = createHash('sha256');
  for (const relativePath of paths.sort()) {
    const bytes = readBounded(checkedPath(root, relativePath, 'file'));
    if (relativePath.endsWith('.json')) JSON.parse(bytes.toString('utf8'));
    hash.update(relativePath).update('\0').update(bytes).update('\0');
  }
  return { digest: hash.digest('hex'), corpusInstances, files: paths.length };
}

/** Local admission policy for the pinned upstream receipt, NOT schema parity. */
export function assertSuccessfulReceipt(report, corpusInstances) {
  assert(positive(corpusInstances), 'expected-corpus-required');
  assert(object(report) && report.schema === REPORT_SCHEMA, 'unexpected-report-schema');
  assert(typeof report.runId === 'string' && /^[a-f0-9]{64}$/.test(report.runId), 'invalid-run-id');
  assert(report.status === 'passed' && report.zeroUnexplainedFindings === true, 'parity-not-passed');
  assert(Array.isArray(report.findings) && report.findings.length === 0, 'unexplained-findings');
  const counts = report.counts;
  assert(object(counts), 'finding-counts-required');
  for (const field of ['findings', 'differentialFindings', 'emittedFindings', 'structuralFindings', 'typespecOutOfScopeDeclarations']) {
    assert.equal(counts[field], 0, 'unexplained-finding-count');
  }
  assert.equal(counts.findingsTruncated, false, 'truncated-findings');
  for (const field of ['authoredDeclarations', 'generatedDeclarations', 'typespecDeclarations']) {
    assert.equal(counts[field], DECLARATIONS.length, 'inventory-count-mismatch');
  }
  const coverage = report.coverage;
  assert(object(coverage), 'coverage-required');
  for (const field of ['differentialInstanceValidation', 'directDeclarationInventory', 'sourceMutationCheck', 'typespecGeneratedJsonSchemaComparison']) {
    assert.equal(coverage[field], true, 'required-check-not-executed');
  }
  assert.deepEqual(coverage.outOfScopeTypeSpecDeclarations, [], 'out-of-scope-declarations');
  const differential = report.differential;
  assert(object(differential) && !Object.hasOwn(differential, 'disabled'), 'differential-lane-required');
  const summary = differential.summary;
  assert(object(summary), 'differential-summary-required');
  assert(summary.comparedDeclarations === DECLARATIONS.length, 'declaration-coverage-incomplete');
  assert(positive(summary.probesEvaluated) && positive(summary.agreements), 'probe-evidence-required');
  assert(summary.divergences === 0 && summary.refusals === 0, 'differential-disagreement');
  assert.equal(summary.agreements, summary.probesEvaluated, 'probe-total-mismatch');
  assert.equal(summary.behaviorallyIndistinguishableDeclarations, DECLARATIONS.length, 'agreement-coverage-incomplete');
  assert(summary.corpusInstances === corpusInstances, 'corpus-coverage-incomplete');
  const rows = differential.declarations;
  assert(Array.isArray(rows) && rows.length === DECLARATIONS.length, 'declaration-coverage-incomplete');
  for (const lane of ['authored', 'generated']) {
    assert.deepEqual(rows.map((row) => row?.[lane]).sort(), DECLARATIONS, 'declaration-identity-mismatch');
  }
  for (const row of rows) {
    assert.equal(row.typespec, `Ores.Middleware.Docs.${row.authored}`, 'typespec-identity-mismatch');
    assert.equal(row.generated, row.authored, 'crossed-declaration-mapping');
    assert(positive(row.probes), 'per-declaration-probes-required');
    assert(row.divergences === 0 && row.refusals === 0, 'per-declaration-disagreement');
    assert(row.behaviorallyIndistinguishable === true, 'per-declaration-agreement-required');
  }
  assert.equal(rows.reduce((total, row) => total + row.probes, 0), summary.probesEvaluated, 'probe-total-mismatch');
}

function invoke(execute, command, args, cwd) {
  const result = execute(command, args, {
    cwd, env: childEnvironment(), encoding: 'utf8', shell: false,
    timeout: 120_000, killSignal: 'SIGKILL', maxBuffer: 1024 * 1024,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  // Do not dump stdout/stderr: compiler output may contain source data.
  assert(result && !result.error && !result.signal && result.status === 0, 'subprocess-failed');
  return result.stdout ?? '';
}

function assertPinnedTool(root, execute) {
  const toolPath = checkedPath(root, 'tmp/tjsv', 'directory');
  const top = invoke(execute, 'git', ['-C', toolPath, 'rev-parse', '--show-toplevel'], root).trim();
  assert(top === toolPath, 'independent-tool-checkout-required');
  const head = invoke(execute, 'git', ['-C', toolPath, 'rev-parse', 'HEAD'], root).trim();
  assert(head === TJSV_REV, 'unexpected-tjsv-revision');
  invoke(execute, 'git', ['-C', toolPath, 'diff', '--quiet', 'HEAD', '--'], root);
  return checkedPath(root, 'tmp/tjsv/bin/typespec-json-schema-validator.mjs', 'file');
}

function freshEvidenceDirectory(root) {
  for (const relativePath of ['target', 'target/tjsv']) {
    try { mkdirSync(join(root, relativePath)); } catch (error) {
      if (error.code !== 'EEXIST') throw error;
    }
    checkedPath(root, relativePath, 'directory');
  }
  return mkdtempSync(join(root, 'target/tjsv/docs-serving-'));
}

/** execute is a unit-test seam; the CLI always uses the real spawnSync. */
export function runDocsGate(root = ROOT, execute = spawnSync) {
  root = realpathSync(root);
  const before = snapshotInputs(root);
  const evidence = freshEvidenceDirectory(root);
  const admissionPath = join(evidence, 'gate.json');
  const receiptPath = join(evidence, 'parity.json');
  const gate = {
    schema: 'ores.middleware.tjsv-docs-gate/v1',
    status: 'failed', scope: 'docs-serving-data-contracts-only',
    toolRevision: TJSV_REV, inputs: before, runtimeEvidence: 'not-evaluated',
  };
  // This is a new private path for THIS invocation, never a reused green receipt.
  writeFileSync(admissionPath, json(gate), { flag: 'wx', mode: 0o600 });
  try {
    const cli = assertPinnedTool(root, execute);
    invoke(execute, process.execPath, [cli, 'check',
      `--typespec=${join(root, 'contracts/docs-serving.tsp')}`,
      `--schema=${join(root, 'contracts/docs-serving.schema.json')}`,
      `--instances=${join(root, CORPUS)}`,
      `--tsp-bin=${join(root, 'tmp/tjsv/node_modules/.bin', process.platform === 'win32' ? 'tsp.cmd' : 'tsp')}`,
      `--report=${receiptPath}`,
      `--output-dir=${join(evidence, 'generated')}`,
      '--probes=true', '--max-probes=64', '--seal-object-schemas=false',
      '--int64-strategy=string', '--polymorphic-models-strategy=oneOf',
      '--format-assertion=false', '--quiet=true',
    ], root);
    assert.deepEqual(snapshotInputs(root), before, 'inputs-changed-during-check');
    const report = JSON.parse(readBounded(receiptPath, MAX_REPORT_BYTES).toString('utf8'));
    assertSuccessfulReceipt(report, before.corpusInstances);
    // Pin and tracked contents must also remain unchanged during compilation.
    assertPinnedTool(root, execute);
    gate.status = 'passed';
    gate.runId = report.runId;
    writeFileSync(admissionPath, json(gate), { mode: 0o600 });
    return { ...gate, evidence };
  } catch (error) {
    // The prewritten failure marker remains authoritative; do not reuse any old run.
    throw new Error('tjsv-docs-gate-failed', { cause: error });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    assert(process.argv.length === 2, 'this-repository-task-accepts-no-cli-options');
    console.log(json(runDocsGate()).trim());
  } catch {
    console.error(json({ status: 'failed', code: 'tjsv-docs-gate-failed' }).trim());
    process.exitCode = 1;
  }
}
