import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

// Fixed entrypoint: no independent argv parser or network/package fallback.
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const toolRoot = resolve(root, process.env.TJSV_ROOT ?? 'target/tooling/tjsv');
const output = resolve(root, 'target/tjsv');
const pin = JSON.parse(await readFile(resolve(root, 'contracts/tjsv-toolchain.json'), 'utf8'));
assert.match(pin.commit, /^[0-9a-f]{40}$/);
assert.equal(execFileSync('git', ['-C', toolRoot, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(), pin.commit);
execFileSync('git', ['-C', toolRoot, 'diff', '--exit-code', 'HEAD', '--'], { stdio: 'pipe' });
const { runCheck, writeReport, buildContractIr, verifyContractIr, writeContractIr } =
  await import(pathToFileURL(resolve(toolRoot, 'src/index.mjs')).href);
const corpus = JSON.parse(await readFile(resolve(root, 'contracts/fixtures/tjsv-instances.json'), 'utf8'));
const replay = JSON.parse(await readFile(resolve(root, 'contracts/fixtures/idempotency-scope.json'), 'utf8'));
const scopes = [
  { name: 'docs-serving', declarations: ['DocsAction', 'DocsDecision', 'DocsRepresentation', 'DocsRequest'], drift: ['DocsRequest', 'method'] },
  { name: 'idempotency-scope', declarations: ['IdempotencyScope'], drift: ['IdempotencyScope', 'serviceName'] }
];
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const results = [];
await mkdir(output, { recursive: true });
let failed = false;
for (const scope of scopes) {
  try {
    const typespec = resolve(root, `contracts/${scope.name}.tsp`);
    const authoredSchema = resolve(root, `contracts/${scope.name}.schema.json`);
    const before = await Promise.all([typespec, authoredSchema].map(path => readFile(path)));
    const instances = resolve(output, scope.name, 'instances');
    const entries = structuredClone(corpus[scope.name]);
    if (scope.name === 'idempotency-scope') {
      entries.IdempotencyScope.valid.push(...replay.cases.map(item => item.scope));
    }
    assert.deepEqual(Object.keys(entries).sort(), [...scope.declarations].sort());
    let corpusCount = 0;
    for (const declaration of scope.declarations) {
      for (const expectation of ['valid', 'invalid']) {
        const values = entries[declaration][expectation];
        assert.ok(Array.isArray(values) && values.length > 0, `${declaration}/${expectation} needs independent examples`);
        const directory = resolve(instances, declaration, expectation);
        await mkdir(directory, { recursive: true });
        for (const [index, value] of values.entries()) {
          await writeFile(resolve(directory, `${index}.json`), `${JSON.stringify(value)}\n`);
          corpusCount++;
        }
      }
    }
    const options = {
      typespec, authoredSchema, instances, probes: true, maxProbes: 128,
      maxFindings: 1000,
      // Both peer authorities already express their own object-closure policy.
      // Do not let the comparison emitter inject an extra unevaluatedProperties
      // assertion: generated Schema B is evidence only, never a third authority.
      sealObjectSchemas: false,
      outputDir: resolve(output, scope.name, 'generated')
    };
    const report = await runCheck(options);
    await writeReport(resolve(output, scope.name, 'report.json'), report);
    assert.equal(report.status, 'passed', `${scope.name}: ${report.status}`);
    assert.equal(report.zeroUnexplainedFindings, true);
    assert.equal(report.findings.length, 0);
    assert.equal(report.counts.typespecDeclarations, scope.declarations.length);
    assert.equal(report.counts.authoredDeclarations, scope.declarations.length);
    assert.equal(report.counts.generatedDeclarations, scope.declarations.length);
    assert.equal(report.coverage.differentialInstanceValidation, true);
    assert.deepEqual(report.coverage.outOfScopeTypeSpecDeclarations, []);
    assert.ok(report.differential.summary.probesEvaluated > 0);
    assert.equal(report.differential.summary.corpusInstances, corpusCount);
    assert.equal(report.differential.summary.divergences, 0);
    assert.equal(report.differential.summary.refusals, 0);
    const evidence = {
      report, typespec, authoredSchema,
      generatedSchema: resolve(options.outputDir, 'typespec.generated.schema.json')
    };
    const ir = await buildContractIr(evidence);
    await writeContractIr(resolve(output, scope.name, 'contract-ir.json'), ir);
    assert.equal(ir.admissible, true);
    const verification = await verifyContractIr({ ...evidence, contractIr: ir });
    assert.equal(verification.status, 'passed');
    assert.equal(verification.admissible, true);
    await writeFile(resolve(output, scope.name, 'ir-verification.json'), JSON.stringify(verification, null, 2));

    // Negative control modifies only a disposable witness, never either peer.
    const drift = JSON.parse(before[1]);
    drift.$defs[scope.drift[0]].properties[scope.drift[1]] = { type: 'integer' };
    const driftSchema = resolve(output, scope.name, 'intentional-drift.schema.json');
    await writeFile(driftSchema, JSON.stringify(drift, null, 2));
    const rejected = await runCheck({ ...options, authoredSchema: driftSchema, outputDir: resolve(output, scope.name, 'drift-generated') });
    await writeReport(resolve(output, scope.name, 'intentional-drift-report.json'), rejected);
    assert.equal(rejected.status, 'stopped_for_evaluation', 'a tool crash is not evidence of drift rejection');
    assert.ok(rejected.differential.summary.divergences > 0);
    assert.ok(rejected.findings.some(item => item.ruleId === 'instance-verdict-divergence'));
    const after = await Promise.all([typespec, authoredSchema].map(path => readFile(path)));
    assert.deepEqual(after.map(digest), before.map(digest), 'peer authorities changed');
    results.push({ scope: scope.name, status: 'passed', runId: report.runId, irId: ir.irId, corpusCount, probes: report.differential.summary.probesEvaluated, negativeControl: rejected.status, sourceDigests: before.map(digest) });
  } catch (error) {
    failed = true;
    results.push({ scope: scope.name, status: 'failed', error: error.message });
  }
}
execFileSync('git', ['-C', toolRoot, 'diff', '--exit-code', 'HEAD', '--'], { stdio: 'pipe' });
const receipt = { schema: 'ores.middleware.tjsv-gate/v1', status: failed ? 'failed' : 'passed', toolCommit: pin.commit, results };
await writeFile(resolve(output, 'receipt.json'), `${JSON.stringify(receipt, null, 2)}\n`);
console.log(JSON.stringify(receipt));
if (failed) process.exitCode = 1;