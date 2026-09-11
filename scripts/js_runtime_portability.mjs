#!/usr/bin/env node
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';

const startedAt = new Date().toISOString();
const root = path.resolve(process.argv[2] ?? process.cwd());
const outputRoot = path.resolve(
  process.argv[3] ?? path.join(root, 'target', 'js-runtime-portability'),
);
const fixturePath = path.join(root, 'fixtures', 'generated-runtime-conformance.json');
const witnessPath = path.join(root, 'tests', 'generated-runtime', 'typescript-witness.mjs');
const docsSmokePath = path.join(root, 'tests', 'runtime-portability', 'docs-serving-smoke.mjs');
const authorities = ['typespec', 'json-schema-openapi'];
const runtimes = [
  {
    name: 'node',
    command: [process.execPath],
    versionCommand: [process.execPath, '--version'],
  },
  {
    name: 'bun',
    command: [process.env.ORES_BUN_BIN ?? 'bun'],
    versionCommand: [process.env.ORES_BUN_BIN ?? 'bun', '--version'],
  },
  {
    name: 'deno',
    command: [process.env.ORES_DENO_BIN ?? 'deno', 'run', '--allow-read'],
    versionCommand: [process.env.ORES_DENO_BIN ?? 'deno', '--version'],
  },
];

const checks = [];
const discrepancies = [];
const runtimeWitnesses = [];
const smokeWitnesses = [];
const runtimeVersions = {};

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === 'object') {
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((key) => [key, canonical(value[key])]),
    );
  }
  return value;
}

function stable(value) {
  return JSON.stringify(canonical(value));
}

function sha256(value) {
  return createHash('sha256').update(value).digest('hex');
}

async function sha256File(filePath) {
  return sha256(await readFile(filePath));
}

async function ensureDirectory(directory) {
  await mkdir(directory, { recursive: true });
}

async function writeJson(filePath, value) {
  await ensureDirectory(path.dirname(filePath));
  await writeFile(filePath, `${JSON.stringify(value, null, 2)}\n`, 'utf8');
}

function finding(kind, detail, cell = null) {
  const item = {
    kind,
    detail,
    fingerprint: sha256(Buffer.from(`${kind}\0${cell ?? ''}\0${detail}`)),
  };
  if (cell) item.cell = cell;
  discrepancies.push(item);
}

async function runCommand(id, command) {
  const logBase = path.join(outputRoot, 'logs', id);
  await ensureDirectory(path.dirname(logBase));
  const started = Date.now();
  const child = spawn(command[0], command.slice(1), {
    cwd: root,
    env: { ...process.env, CI: 'true', NO_COLOR: '1' },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let stdout = '';
  let stderr = '';
  child.stdout.setEncoding('utf8');
  child.stderr.setEncoding('utf8');
  child.stdout.on('data', (chunk) => {
    stdout += chunk;
  });
  child.stderr.on('data', (chunk) => {
    stderr += chunk;
  });
  const exitCode = await new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', resolve);
  });
  await writeFile(`${logBase}.stdout.log`, stdout, 'utf8');
  await writeFile(`${logBase}.stderr.log`, stderr, 'utf8');
  checks.push({
    id,
    command,
    exitCode,
    durationMs: Date.now() - started,
    stdoutLog: path.relative(root, `${logBase}.stdout.log`),
    stderrLog: path.relative(root, `${logBase}.stderr.log`),
  });
  if (exitCode !== 0) {
    throw new Error(`${id} failed with exit code ${exitCode}: ${command.join(' ')}`);
  }
  return { stdout, stderr };
}

function parseLastJsonLine(stdout, cell) {
  const candidates = stdout
    .split(/\r?\n/u)
    .map((line) => line.trim())
    .filter((line) => line.startsWith('{') && line.endsWith('}'));
  if (candidates.length === 0) throw new Error(`${cell} emitted no JSON result`);
  return JSON.parse(candidates.at(-1));
}

function normalizedWitness(witness) {
  return {
    schema: witness.schema,
    language: witness.language,
    model: witness.model,
    wireFields: witness.wireFields,
    requiredFields: witness.requiredFields,
    optionalFields: witness.optionalFields,
    statuses: witness.statuses,
    statusAcceptance: witness.statusAcceptance,
    cases: witness.cases,
  };
}

function expectedCases(fixture) {
  return fixture.cases.map((testCase) => ({
    id: testCase.id,
    accepted: testCase.expect === 'accept',
    normalized: testCase.expect === 'accept'
      ? Object.fromEntries(
          fixture.wireFields
            .filter((field) => Object.hasOwn(testCase.value, field))
            .map((field) => [field, testCase.value[field]]),
        )
      : null,
  }));
}

function validateAgainstFixture(witness, fixture, authority, runtime) {
  const cell = `${authority}/typescript/${runtime}`;
  const expected = {
    schema: 'ores.generated-runtime-witness/v1',
    language: 'typescript',
    model: fixture.model,
    wireFields: fixture.wireFields,
    requiredFields: fixture.requiredFields,
    optionalFields: fixture.optionalFields,
    statuses: fixture.statuses,
    statusAcceptance: Object.fromEntries(
      [...fixture.statuses, '__unknown__'].map((status) => [
        status,
        fixture.statuses.includes(status),
      ]),
    ),
    cases: expectedCases(fixture),
  };
  if (witness.authority !== authority) {
    finding('authority-mismatch', `actual=${witness.authority} expected=${authority}`, cell);
  }
  if (stable(normalizedWitness(witness)) !== stable(expected)) {
    finding(
      'fixture-semantic-mismatch',
      `actual=${stable(normalizedWitness(witness))} expected=${stable(expected)}`,
      cell,
    );
  }
}

async function recordRuntimeVersions() {
  for (const runtime of runtimes) {
    try {
      const result = await runCommand(`versions/${runtime.name}`, runtime.versionCommand);
      runtimeVersions[runtime.name] = result.stdout.trim().split(/\r?\n/u)[0];
    } catch (error) {
      runtimeVersions[runtime.name] = null;
      finding('runtime-version-failure', error.message, runtime.name);
    }
  }
}

function generatedPath(authority) {
  return path.join(
    root,
    'target',
    'schema-convergence',
    authority,
    'typescript',
    'idempotency_record.mjs',
  );
}

async function runDocsSmoke() {
  for (const runtime of runtimes) {
    const cell = `docs-serving/${runtime.name}`;
    try {
      const result = await runCommand(
        `${cell}/runtime`,
        [...runtime.command, docsSmokePath],
      );
      const witness = parseLastJsonLine(result.stdout, cell);
      if (
        witness.schema !== 'ores.middleware.js-runtime-portability-smoke/v1' ||
        witness.suite !== 'docs-serving' ||
        witness.status !== 'passed'
      ) {
        finding('docs-serving-smoke-mismatch', stable(witness), cell);
        smokeWitnesses.push({ runtime: runtime.name, state: 'discrepant', witness });
      } else {
        smokeWitnesses.push({ runtime: runtime.name, state: 'executed', witness });
      }
    } catch (error) {
      finding('docs-serving-smoke-failure', error.message, cell);
      smokeWitnesses.push({ runtime: runtime.name, state: 'failed', error: error.message });
    }
  }
}

async function runGeneratedWitnesses(fixture) {
  const baselines = new Map();
  for (const authority of authorities) {
    const artifact = generatedPath(authority);
    const artifactSha256 = await sha256File(artifact);
    for (const runtime of runtimes) {
      const cell = `${authority}/typescript/${runtime.name}`;
      try {
        const result = await runCommand(
          `${cell}/runtime`,
          [...runtime.command, witnessPath, fixturePath, artifact, authority],
        );
        const witness = parseLastJsonLine(result.stdout, cell);
        const before = discrepancies.length;
        validateAgainstFixture(witness, fixture, authority, runtime.name);
        const resultPath = path.join(
          outputRoot,
          'results',
          authority,
          `typescript-${runtime.name}.json`,
        );
        await writeJson(resultPath, witness);
        const normalized = normalizedWitness(witness);
        if (runtime.name === 'node') {
          baselines.set(authority, normalized);
        } else {
          const baseline = baselines.get(authority);
          if (!baseline || stable(normalized) !== stable(baseline)) {
            finding(
              'js-runtime-semantic-mismatch',
              `runtime=${runtime.name} baseline=node actual=${stable(normalized)} expected=${stable(baseline)}`,
              cell,
            );
          }
        }
        runtimeWitnesses.push({
          authority,
          language: 'typescript',
          runtime: runtime.name,
          state: discrepancies.length === before ? 'executed' : 'discrepant',
          generatedArtifact: path.relative(root, artifact),
          generatedArtifactSha256: artifactSha256,
          result: path.relative(root, resultPath),
          resultSha256: await sha256File(resultPath),
        });
      } catch (error) {
        finding('js-runtime-witness-failure', error.message, cell);
        runtimeWitnesses.push({
          authority,
          language: 'typescript',
          runtime: runtime.name,
          state: 'failed',
          error: error.message,
        });
      }
    }
  }

  const typeSpec = baselines.get('typespec');
  const jsonSchema = baselines.get('json-schema-openapi');
  if (!typeSpec || !jsonSchema || stable(typeSpec) !== stable(jsonSchema)) {
    finding(
      'authority-runtime-semantic-mismatch',
      `typespec=${stable(typeSpec)} json-schema-openapi=${stable(jsonSchema)}`,
      'node/typescript',
    );
  }
}

await rm(outputRoot, { recursive: true, force: true });
await ensureDirectory(outputRoot);
const fixture = JSON.parse(await readFile(fixturePath, 'utf8'));
if (fixture.schema !== 'ores.generated-runtime-conformance/v1') {
  throw new Error(`unsupported fixture schema: ${fixture.schema}`);
}

await recordRuntimeVersions();
await runDocsSmoke();
await runGeneratedWitnesses(fixture);

const expectedRuntimeWitnessCount = authorities.length * runtimes.length;
const passed =
  discrepancies.length === 0 &&
  runtimeWitnesses.length === expectedRuntimeWitnessCount &&
  runtimeWitnesses.every((item) => item.state === 'executed') &&
  smokeWitnesses.length === runtimes.length &&
  smokeWitnesses.every((item) => item.state === 'executed');

const receipt = {
  schema: 'ores.middleware.js-runtime-portability-report/v1',
  repository: 'ORESoftware/ores-middleware',
  commit: process.env.ORES_WORKFLOW_SOURCE_SHA ?? process.env.GITHUB_SHA ?? null,
  startedAt,
  endedAt: new Date().toISOString(),
  authorities,
  language: 'typescript',
  runtimes: runtimes.map((runtime) => ({
    name: runtime.name,
    version: runtimeVersions[runtime.name] ?? null,
  })),
  sourceDigests: {
    [path.relative(root, fixturePath)]: await sha256File(fixturePath),
    [path.relative(root, witnessPath)]: await sha256File(witnessPath),
    [path.relative(root, docsSmokePath)]: await sha256File(docsSmokePath),
  },
  runtimeWitnesses,
  smokeWitnesses,
  checks,
  discrepancies,
  status: passed ? 'passed' : 'failed',
  zeroUnexplainedFindings: passed,
};

const receiptPath = path.join(outputRoot, 'receipt.json');
await writeJson(receiptPath, receipt);
console.log(`js runtime portability status=${receipt.status} receipt=${path.relative(root, receiptPath)}`);
if (!passed) process.exitCode = 1;
