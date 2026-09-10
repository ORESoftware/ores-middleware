#!/usr/bin/env node
/** Validate compiled .ores-mw.toml evidence against both authored JSON contracts. */
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { lstatSync, readFileSync } from 'node:fs';
import { dirname, join, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import Ajv2020 from 'ajv/dist/2020.js';
import addFormats from 'ajv-formats';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const MAX_BYTES = 2 * 1024 * 1024;
const json = (value) => `${JSON.stringify(value, null, 2)}\n`;
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

function checkedFile(relativePath) {
  assert(typeof relativePath === 'string' && relativePath.length > 0, 'path-required');
  assert(!relativePath.startsWith('/') && !relativePath.includes('\\'), 'portable-relative-path-required');
  assert(relativePath.split('/').every((part) => part && part !== '.' && part !== '..'), 'safe-relative-path-required');
  const path = join(ROOT, relativePath);
  assert(path.startsWith(`${ROOT}${sep}`), 'path-outside-repository');
  const stat = lstatSync(path);
  assert(stat.isFile() && !stat.isSymbolicLink() && stat.nlink === 1, 'independent-regular-file-required');
  assert(stat.size <= MAX_BYTES, 'file-too-large');
  return path;
}

function readJson(relativePath) {
  const path = checkedFile(relativePath);
  const bytes = readFileSync(path);
  assert(bytes.length <= MAX_BYTES, 'file-too-large');
  return { bytes, value: JSON.parse(bytes.toString('utf8')) };
}

function validator(schema) {
  const ajv = new Ajv2020({ allErrors: true, strict: true });
  addFormats(ajv);
  return ajv.compile(schema);
}

function requireValid(validate, value, code) {
  if (!validate(value)) throw new Error(code);
}

export function run(root = ROOT) {
  assert.equal(root, ROOT, 'repository-root-only');
  const manifestSchema = readJson('contracts/ores-mw-config/json-schema/ores-mw-config.schema.json').value;
  const stackSchema = readJson('contracts/json-schema/middleware-stack.schema.json').value;
  const manifest = readJson('target/ores-mw/manifest.json');
  const receipt = readJson('target/ores-mw/receipt.json').value;

  assert.equal(receipt?.schema, 'ores.middleware.config-receipt/v1', 'unexpected-receipt-schema');
  assert.equal(receipt?.status, 'passed', 'compile-receipt-not-passed');
  assert.match(receipt?.normalizedManifestSha256 ?? '', /^[0-9a-f]{64}$/, 'manifest-digest-required');
  assert.equal(receipt.normalizedManifestSha256, sha256(manifest.bytes), 'compiled-manifest-digest-mismatch');

  const validateManifest = validator(manifestSchema);
  const validateStack = validator(stackSchema);
  requireValid(validateManifest, manifest.value, 'normalized-manifest-schema-invalid');
  assert.equal(receipt.targetCount, manifest.value.targets.length, 'target-count-mismatch');
  assert.equal(receipt.targets.length, manifest.value.targets.length, 'receipt-target-count-mismatch');

  const receiptByName = new Map(receipt.targets.map((target) => [target.name, target]));
  assert.equal(receiptByName.size, receipt.targets.length, 'duplicate-receipt-target');
  let stackTargets = 0;
  for (const target of manifest.value.targets) {
    const evidence = receiptByName.get(target.name);
    assert(evidence, 'target-evidence-required');
    assert.equal(evidence.role, target.role, 'target-role-mismatch');
    assert.equal(evidence.enabled, target.enabled, 'target-enabled-mismatch');
    assert.equal(evidence.middleware, target.middleware, 'target-middleware-mismatch');
    assert.deepEqual(evidence.roots, target.roots, 'target-roots-mismatch');
    if (target.middleware !== 'stack') continue;

    stackTargets += 1;
    assert.equal(target.role, 'server', 'full-stack-is-server-only');
    assert.equal(evidence.stackConfig, target.stackConfig, 'stack-config-path-mismatch');
    assert.match(evidence.stackConfigSourceSha256 ?? '', /^[0-9a-f]{64}$/, 'source-stack-digest-required');
    assert.match(evidence.stackConfigNormalizedSha256 ?? '', /^[0-9a-f]{64}$/, 'normalized-stack-digest-required');
    assert.equal(evidence.compiledPath, `targets/${target.name}.json`, 'unexpected-compiled-path');
    const compiled = readJson(`target/ores-mw/${evidence.compiledPath}`);
    assert.equal(evidence.stackConfigNormalizedSha256, sha256(compiled.bytes), 'compiled-stack-digest-mismatch');
    requireValid(validateStack, compiled.value, 'compiled-stack-schema-invalid');
  }

  assert(stackTargets > 0, 'repository-stack-target-required');
  return {
    schema: 'ores.middleware.config-json-admission/v1',
    status: 'passed',
    targetCount: manifest.value.targets.length,
    stackTargets,
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    assert.equal(process.argv.length, 2, 'this-repository-task-accepts-no-cli-options');
    console.log(json(run()).trim());
  } catch {
    console.error(json({ status: 'failed', code: 'ores-mw-config-json-admission-failed' }).trim());
    process.exitCode = 1;
  }
}
