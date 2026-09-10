#!/usr/bin/env node
/** Fail-closed TJSV gate for the normalized .ores-mw.toml orchestration contract. */
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, writeFileSync,
} from 'node:fs';
import { dirname, join, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

export const TJSV_REV = '4a5d049218adc2740d4cf78f612caf7f38f6f64c';
export const DECLARATIONS = Object.freeze(['MiddlewareManifest', 'MiddlewareTarget']);
const REPORT_SCHEMA = 'ores.typespec-json-schema-validator.report/v1';
const CORPUS = 'contracts/tjsv-instances/ores-mw-config';
const TYPESPEC = 'contracts/ores-mw-config/typespec/main.tsp';
const JSON_SCHEMA = 'contracts/ores-mw-config/json-schema/ores-mw-config.schema.json';
const MAX_FILE_BYTES = 256 * 1024;
const MAX_REPORT_BYTES = 4 * 1024 * 1024;
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const object = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
const positive = (value) => Number.isSafeInteger(value) && value > 0;
const json = (value) => `${JSON.stringify(value, null, 2)}\n`;

/** Subprocesses inherit only process-lifecycle paths, never ambient validator flags or credentials. */
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

/** Hash both authored authorities and every positive/negative declaration fixture. */
export function snapshotInputs(root) {
  const corpusPath = checkedPath(root, CORPUS, 'directory');
  assert.deepEqual(readdirSync(corpusPath).sort(), [...DECLARATIONS].sort(), 'unexpected-corpus-declaration');
  const paths = [TYPESPEC, JSON_SCHEMA];
  let corpusInstances = 0;
  for (const declaration of DECLARATIONS) {
    const declarationPath = checkedPath(root, `${CORPUS}/${declaration}`, 'directory');
    assert.deepEqual(readdirSync(declarationPath).sort(), ['invalid', 'valid'], 'expectation-lanes-required');
    for (const lane of ['invalid', 'valid']) {
      const relativeDirectory = `${CORPUS}/${declaration}/${lane}`;
      const directory = checkedPath(root, relativeDirectory, 'directory');
      const names = readdirSync(directory).sort();
      assert(names.length > 0, 'empty-expectation-lane');
      for (const filename of names) {
        assert(/^[a-z0-9][a-z0-9-]*\.json$/.test(filename), 'invalid-fixture-filename');
        const relativePath = `${relativeDirectory}/${filename}`;
        JSON.parse(readBounded(checkedPath(root, relativePath, 'file')).toString('utf8'));
        paths.push(relativePath);
        corpusInstances += 1;
        assert(corpusInstances <= 128, 'too-many-fixtures');
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

/** Local admission policy for an actual pinned TJSV receipt. */
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
  assert.equal(summary.comparedDeclarations, DECLARATIONS.length, 'declaration-coverage-incomplete');
  assert(positive(summary.probesEvaluated) && positive(summary.agreements), 'probe-evidence-required');
  assert.equal(summary.divergences, 0, 'differential-divergence');
  assert.equal(summary.refusals, 0, 'differential-refusal');
  assert.equal(summary.agreements, summary.probesEvaluated, 'probe-total-mismatch');
  assert.equal(summary.behaviorallyIndistinguishableDeclarations, DECLARATIONS.length, 'agreement-coverage-incomplete');
  assert.equal(summary.corpusInstances, corpusInstances, 'corpus-coverage-incomplete');

  const rows = differential.declarations;
  assert(Array.isArray(rows) && rows.length === DECLARATIONS.length, 'declaration-coverage-incomplete');
  assert.deepEqual(rows.map((row) => row?.authored).sort(), [...DECLARATIONS].sort(), 'authored-identity-mismatch');
  assert.deepEqual(rows.map((row) => row?.generated).sort(), [...DECLARATIONS].sort(), 'generated-identity-mismatch');
  for (const row of rows) {
    assert.equal(row.typespec, `Ores.Middleware.Config.${row.authored}`, 'typespec-identity-mismatch');
    assert.equal(row.generated, row.authored, 'crossed-declaration-mapping');
    assert(positive(row.probes), 'per-declaration-probes-required');
    assert.equal(row.divergences, 0, 'per-declaration-divergence');
    assert.equal(row.refusals, 0, 'per-declaration-refusal');
    assert.equal(row.behaviorallyIndistinguishable, true, 'per-declaration-agreement-required');
  }
  assert.equal(rows.reduce((total, row) => total + row.probes, 0), summary.probesEvaluated, 'probe-total-mismatch');
}

function invoke(execute, command, args, cwd) {
  const result = execute(command, args, {
    cwd,
    env: childEnvironment(),
    encoding: 'utf8',
    shell: false,
    timeout: 120_000,
    killSignal: 'SIGKILL',
    maxBuffer: 1024 * 1024,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  assert(result && !result.error && !result.signal && result.status === 0, 'subprocess-failed');
  return result.stdout ?? '';
}

function assertPinnedTool(root, execute) {
  const toolPath = checkedPath(root, 'target/tools/tjsv', 'directory');
  const top = invoke(execute, 'git', ['-C', toolPath, 'rev-parse', '--show-toplevel'], root).trim();
  assert.equal(top, toolPath, 'independent-tool-checkout-required');
  const head = invoke(execute, 'git', ['-C', toolPath, 'rev-parse', 'HEAD'], root).trim();
  assert.equal(head, TJSV_REV, 'unexpected-tjsv-revision');
  invoke(execute, 'git', ['-C', toolPath, 'diff', '--quiet', 'HEAD', '--'], root);
  return checkedPath(root, 'target/tools/tjsv/bin/typespec-json-schema-validator.mjs', 'file');
}

function freshEvidenceDirectory(root) {
  for (const relativePath of ['target', 'target/tjsv']) {
    try { mkdirSync(join(root, relativePath)); } catch (error) {
      if (error.code !== 'EEXIST') throw error;
    }
    const stat = lstatSync(join(root, relativePath));
    assert(stat.isDirectory() && !stat.isSymbolicLink(), 'evidence-directory-required');
  }
  return mkdtempSync(join(root, 'target/tjsv/ores-mw-config-'));
}

export function runOresMwGate(root = ROOT, execute = spawnSync) {
  root = realpathSync(root);
  const before = snapshotInputs(root);
  const evidence = freshEvidenceDirectory(root);
  const admissionPath = join(evidence, 'gate.json');
  const receiptPath = join(evidence, 'parity.json');
  const gate = {
    schema: 'ores.middleware.tjsv-ores-mw-config-gate/v1',
    status: 'failed',
    scope: 'normalized-ores-mw-config-contract-only',
    toolRevision: TJSV_REV,
    inputs: before,
  };
  writeFileSync(admissionPath, json(gate), { flag: 'wx', mode: 0o600 });
  try {
    const cli = assertPinnedTool(root, execute);
    invoke(execute, process.execPath, [cli, 'check',
      `--typespec=${join(root, TYPESPEC)}`,
      `--schema=${join(root, JSON_SCHEMA)}`,
      `--instances=${join(root, CORPUS)}`,
      `--tsp-bin=${join(root, 'target/tools/tjsv/node_modules/.bin', process.platform === 'win32' ? 'tsp.cmd' : 'tsp')}`,
      `--report=${receiptPath}`,
      `--output-dir=${join(evidence, 'generated')}`,
      '--probes=true', '--max-probes=64', '--seal-object-schemas=false',
      '--int64-strategy=string', '--polymorphic-models-strategy=oneOf',
      '--format-assertion=false', '--quiet=true',
    ], root);
    assert.deepEqual(snapshotInputs(root), before, 'inputs-changed-during-check');
    const report = JSON.parse(readBounded(receiptPath, MAX_REPORT_BYTES).toString('utf8'));
    assertSuccessfulReceipt(report, before.corpusInstances);
    assertPinnedTool(root, execute);
    gate.status = 'passed';
    gate.runId = report.runId;
    writeFileSync(admissionPath, json(gate), { mode: 0o600 });
    return { ...gate, evidence };
  } catch (error) {
    throw new Error('tjsv-ores-mw-config-gate-failed', { cause: error });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    assert.equal(process.argv.length, 2, 'this-repository-task-accepts-no-cli-options');
    console.log(json(runOresMwGate()).trim());
  } catch {
    console.error(json({ status: 'failed', code: 'tjsv-ores-mw-config-gate-failed' }).trim());
    process.exitCode = 1;
  }
}
