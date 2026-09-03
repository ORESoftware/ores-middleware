#!/usr/bin/env node
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { cp, mkdir, readFile, readdir, rm, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';

const startedAt = new Date().toISOString();
const root = path.resolve(process.argv[2] ?? process.cwd());
const outputRoot = path.resolve(
  process.argv[3] ?? path.join(root, 'target', 'generated-runtime-convergence'),
);
const fixturePath = path.join(root, 'fixtures', 'generated-runtime-conformance.json');
const receiptPath = path.join(outputRoot, 'receipt.json');
const lanes = ['typespec', 'json-schema-openapi'];
const languages = ['typescript', 'rust', 'golang', 'gleam', 'elixir', 'erlang'];
const generatedRelative = {
  typescript: 'typescript/idempotency_record.mjs',
  rust: 'rust/idempotency_record.rs',
  golang: 'golang/idempotency_record.go',
  gleam: 'gleam/idempotency_record.gleam',
  elixir: 'elixir/idempotency_record.ex',
  erlang: 'erlang/idempotency_record.erl',
};
const discrepancies = [];
const witnesses = [];
const checks = [];

class CommandFailure extends Error {
  constructor(id, command, code, stdout, stderr) {
    super(`${id} failed with exit code ${code}: ${command.join(' ')}`);
    this.name = 'CommandFailure';
    this.id = id;
    this.command = command;
    this.code = code;
    this.stdout = stdout;
    this.stderr = stderr;
  }
}

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

function sha256Buffer(buffer) {
  return createHash('sha256').update(buffer).digest('hex');
}

async function sha256File(filePath) {
  return sha256Buffer(await readFile(filePath));
}

function finding(kind, detail, cell = null) {
  const fingerprint = sha256Buffer(Buffer.from(`${kind}\0${cell ?? ''}\0${detail}`));
  const item = {
    fingerprint,
    kind,
    detail,
    resolutionState: 'unexplained',
  };
  if (cell) item.cell = cell;
  discrepancies.push(item);
  return item;
}

async function ensureDirectory(directory) {
  await mkdir(directory, { recursive: true });
}

async function writeJson(filePath, value) {
  await ensureDirectory(path.dirname(filePath));
  await writeFile(filePath, `${JSON.stringify(value, null, 2)}\n`, 'utf8');
}

function commandEnvironment(extra = {}) {
  return {
    ...process.env,
    CI: 'true',
    NO_COLOR: '1',
    CARGO_TERM_COLOR: 'never',
    ...extra,
  };
}

async function runCommand(id, command, options = {}) {
  const cwd = options.cwd ?? root;
  const logBase = path.join(outputRoot, 'logs', id);
  await ensureDirectory(path.dirname(logBase));
  const started = Date.now();
  const child = spawn(command[0], command.slice(1), {
    cwd,
    env: commandEnvironment(options.env),
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
  const code = await new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', resolve);
  });
  await writeFile(`${logBase}.stdout.log`, stdout, 'utf8');
  await writeFile(`${logBase}.stderr.log`, stderr, 'utf8');
  const record = {
    id,
    command,
    cwd: path.relative(root, cwd) || '.',
    exitCode: code,
    durationMs: Date.now() - started,
    stdoutLog: path.relative(root, `${logBase}.stdout.log`),
    stderrLog: path.relative(root, `${logBase}.stderr.log`),
    state: code === 0 ? 'executed' : 'failed',
  };
  checks.push(record);
  if (code !== 0) {
    throw new CommandFailure(id, command, code, stdout, stderr);
  }
  return { stdout, stderr, record };
}

function parseWitness(stdout, cell) {
  const candidates = stdout
    .split(/\r?\n/u)
    .map((line) => line.trim())
    .filter((line) => line.startsWith('{') && line.endsWith('}'));
  if (candidates.length === 0) {
    throw new Error(`${cell} did not emit a JSON witness`);
  }
  return JSON.parse(candidates.at(-1));
}

function sortedStrings(value) {
  return [...value].sort((left, right) => left.localeCompare(right));
}

function compare(label, actual, expected, kind, cell) {
  if (stable(actual) !== stable(expected)) {
    finding(
      kind,
      `${label}: actual=${stable(actual)} expected=${stable(expected)}`,
      cell,
    );
  }
}

function expectedCaseMap(fixture) {
  return new Map(fixture.cases.map((item) => [item.id, item]));
}

function validateWitness(witness, fixture, authority, language) {
  const cell = `${authority}/${language}`;
  compare('witness schema', witness.schema, 'ores.generated-runtime-witness/v1', 'witness-schema-mismatch', cell);
  compare('authority', witness.authority, authority, 'witness-authority-mismatch', cell);
  compare('language', witness.language, language, 'witness-language-mismatch', cell);
  compare('model', witness.model, fixture.model, 'witness-model-mismatch', cell);
  compare('wire fields', sortedStrings(witness.wireFields ?? []), sortedStrings(fixture.wireFields), 'wire-field-mismatch', cell);
  compare('required fields', sortedStrings(witness.requiredFields ?? []), sortedStrings(fixture.requiredFields), 'required-field-mismatch', cell);
  compare('optional fields', sortedStrings(witness.optionalFields ?? []), sortedStrings(fixture.optionalFields), 'optional-field-mismatch', cell);
  compare('statuses', witness.statuses ?? [], fixture.statuses, 'status-vocabulary-mismatch', cell);

  const expectedStatusAcceptance = Object.fromEntries([
    ...fixture.statuses.map((status) => [status, true]),
    ['__unknown__', false],
  ]);
  compare('status acceptance', witness.statusAcceptance ?? {}, expectedStatusAcceptance, 'status-runtime-mismatch', cell);

  const expectedCases = expectedCaseMap(fixture);
  const actualCases = new Map((witness.cases ?? []).map((item) => [item.id, item]));
  compare('case IDs', sortedStrings(actualCases.keys()), sortedStrings(expectedCases.keys()), 'runtime-case-set-mismatch', cell);
  for (const [id, expected] of expectedCases) {
    const actual = actualCases.get(id);
    if (!actual) continue;
    const accepted = expected.expect === 'accept';
    compare(`${id} acceptance`, actual.accepted, accepted, 'runtime-acceptance-mismatch', cell);
    compare(
      `${id} normalized value`,
      actual.normalized ?? null,
      accepted ? expected.value : null,
      'runtime-roundtrip-mismatch',
      cell,
    );
  }
}

async function copyHarness(source, destination) {
  await rm(destination, { recursive: true, force: true });
  await ensureDirectory(path.dirname(destination));
  await cp(source, destination, { recursive: true });
}

function generatedPath(authority, language) {
  return path.join(
    root,
    'target',
    'schema-convergence',
    authority,
    generatedRelative[language],
  );
}

async function runTypeScript(authority) {
  const result = await runCommand(
    `${authority}/typescript/runtime`,
    [
      process.execPath,
      path.join(root, 'tests/generated-runtime/typescript-witness.mjs'),
      fixturePath,
      generatedPath(authority, 'typescript'),
      authority,
    ],
  );
  return parseWitness(result.stdout, `${authority}/typescript`);
}

async function runGo(authority) {
  const workspace = path.join(outputRoot, 'harness', authority, 'golang');
  await copyHarness(path.join(root, 'tests/generated-runtime/go'), workspace);
  await ensureDirectory(path.join(workspace, 'persistence'));
  await cp(generatedPath(authority, 'golang'), path.join(workspace, 'persistence', 'idempotency_record.go'));
  await runCommand(
    `${authority}/golang/format`,
    ['gofmt', '-w', '.'],
    { cwd: workspace },
  );
  await runCommand(
    `${authority}/golang/test`,
    ['go', 'test', './...'],
    { cwd: workspace, env: { GO111MODULE: 'on' } },
  );
  const result = await runCommand(
    `${authority}/golang/runtime`,
    ['go', 'run', '.', fixturePath, authority],
    { cwd: workspace, env: { GO111MODULE: 'on' } },
  );
  return parseWitness(result.stdout, `${authority}/golang`);
}

async function runRust(authority) {
  const workspace = path.join(outputRoot, 'harness', authority, 'rust');
  await copyHarness(path.join(root, 'tests/generated-runtime/rust'), workspace);
  await cp(generatedPath(authority, 'rust'), path.join(workspace, 'src', 'generated.rs'));
  const manifest = path.join(workspace, 'Cargo.toml');
  const target = path.join(outputRoot, 'cargo-target', authority);
  await runCommand(
    `${authority}/rust/lock`,
    ['cargo', 'generate-lockfile', '--manifest-path', manifest],
    { env: { CARGO_TARGET_DIR: target } },
  );
  await runCommand(
    `${authority}/rust/check`,
    ['cargo', 'check', '--locked', '--all-targets', '--manifest-path', manifest],
    { env: { CARGO_TARGET_DIR: target, RUSTFLAGS: '-Dwarnings' } },
  );
  const result = await runCommand(
    `${authority}/rust/runtime`,
    ['cargo', 'run', '--quiet', '--locked', '--manifest-path', manifest, '--', fixturePath, authority],
    { env: { CARGO_TARGET_DIR: target, RUSTFLAGS: '-Dwarnings' } },
  );
  return parseWitness(result.stdout, `${authority}/rust`);
}

async function ensureElixir() {
  const cwd = path.join(root, 'src', 'elixir');
  await runCommand('shared/elixir/deps', ['mix', 'deps.get'], { cwd });
  await runCommand('shared/elixir/compile', ['mix', 'compile', '--warnings-as-errors'], { cwd });
}

async function runElixir(authority) {
  const cwd = path.join(root, 'src', 'elixir');
  const result = await runCommand(
    `${authority}/elixir/runtime`,
    [
      'mix',
      'run',
      '--no-start',
      path.join(root, 'tests/generated-runtime/elixir_witness.exs'),
      '--',
      fixturePath,
      generatedPath(authority, 'elixir'),
      authority,
    ],
    { cwd },
  );
  return parseWitness(result.stdout, `${authority}/elixir`);
}

async function runErlang(authority) {
  const workspace = path.join(outputRoot, 'harness', authority, 'erlang');
  await copyHarness(path.join(root, 'tests/generated-runtime/erlang'), workspace);
  const generated = path.join(workspace, 'src', 'ores_middleware_generated_idempotency_record.erl');
  await cp(generatedPath(authority, 'erlang'), generated);
  await runCommand(
    `${authority}/erlang/build`,
    ['rebar3', 'escriptize'],
    { cwd: workspace },
  );
  const executable = path.join(workspace, '_build', 'default', 'bin', 'runtime_witness');
  const result = await runCommand(
    `${authority}/erlang/runtime`,
    [executable, fixturePath, authority, generated],
    { cwd: workspace },
  );
  return parseWitness(result.stdout, `${authority}/erlang`);
}

async function runGleam(authority) {
  const workspace = path.join(outputRoot, 'harness', authority, 'gleam');
  await copyHarness(path.join(root, 'tests/generated-runtime/gleam'), workspace);
  const generated = path.join(workspace, 'src', 'idempotency_record.gleam');
  await cp(generatedPath(authority, 'gleam'), generated);
  await runCommand(
    `${authority}/gleam/format`,
    ['gleam', 'format', '--check', 'src'],
    { cwd: workspace },
  );
  const result = await runCommand(
    `${authority}/gleam/runtime`,
    ['gleam', 'run', '-m', 'runtime_witness'],
    {
      cwd: workspace,
      env: {
        ORES_RUNTIME_FIXTURE: fixturePath,
        ORES_RUNTIME_AUTHORITY: authority,
        ORES_RUNTIME_GENERATED_SOURCE: generated,
      },
    },
  );
  return parseWitness(result.stdout, `${authority}/gleam`);
}

const runners = {
  typescript: runTypeScript,
  rust: runRust,
  golang: runGo,
  gleam: runGleam,
  elixir: runElixir,
  erlang: runErlang,
};

async function filesBelow(directory) {
  const output = [];
  async function walk(current) {
    for (const entry of await readdir(current, { withFileTypes: true })) {
      const absolute = path.join(current, entry.name);
      if (entry.isDirectory()) await walk(absolute);
      else if (entry.isFile()) output.push(absolute);
    }
  }
  await walk(directory);
  return output.sort();
}

async function harnessDigests() {
  const directory = path.join(root, 'tests', 'generated-runtime');
  const paths = await filesBelow(directory);
  const values = {};
  for (const filePath of paths) {
    values[path.relative(root, filePath)] = await sha256File(filePath);
  }
  return values;
}

async function requiredFileDigest(filePath) {
  try {
    const metadata = await stat(filePath);
    if (!metadata.isFile()) throw new Error('not a file');
    return await sha256File(filePath);
  } catch (error) {
    finding('missing-required-artifact', `${path.relative(root, filePath)}: ${error.message}`);
    return null;
  }
}

async function main() {
  await rm(outputRoot, { recursive: true, force: true });
  await ensureDirectory(outputRoot);
  const fixture = JSON.parse(await readFile(fixturePath, 'utf8'));
  if (fixture.schema !== 'ores.generated-runtime-conformance/v1') {
    throw new Error(`unsupported fixture schema: ${fixture.schema}`);
  }
  if (fixture.authority !== 'independent-fixture-corpus') {
    throw new Error('fixture corpus must declare independent-fixture-corpus authority');
  }

  let elixirReady = true;
  try {
    await ensureElixir();
  } catch (error) {
    elixirReady = false;
    finding('runtime-toolchain-preflight-failure', error.message, 'shared/elixir');
  }

  for (const authority of lanes) {
    for (const language of languages) {
      const cell = `${authority}/${language}`;
      if (language === 'elixir' && !elixirReady) {
        witnesses.push({ authority, language, state: 'failed', error: 'Elixir preflight failed' });
        continue;
      }
      try {
        const artifact = generatedPath(authority, language);
        const artifactSha256 = await requiredFileDigest(artifact);
        if (!artifactSha256) {
          witnesses.push({ authority, language, state: 'failed', error: 'generated artifact missing' });
          continue;
        }
        const before = discrepancies.length;
        const witness = await runners[language](authority);
        validateWitness(witness, fixture, authority, language);
        const resultPath = path.join(outputRoot, 'results', authority, `${language}.json`);
        await writeJson(resultPath, witness);
        witnesses.push({
          authority,
          language,
          state: discrepancies.length === before ? 'executed' : 'discrepant',
          generatedArtifact: path.relative(root, artifact),
          generatedArtifactSha256: artifactSha256,
          result: path.relative(root, resultPath),
          resultSha256: await sha256File(resultPath),
        });
      } catch (error) {
        finding('runtime-witness-execution-failure', error.message, cell);
        witnesses.push({
          authority,
          language,
          state: 'failed',
          error: error.message,
        });
      }
    }
  }

  const successful = witnesses.filter((item) => item.state === 'executed');
  if (successful.length > 0) {
    const baselinePath = path.join(root, successful[0].result);
    const baseline = JSON.parse(await readFile(baselinePath, 'utf8'));
    const baselineSemantics = {
      model: baseline.model,
      wireFields: sortedStrings(baseline.wireFields),
      requiredFields: sortedStrings(baseline.requiredFields),
      optionalFields: sortedStrings(baseline.optionalFields),
      statuses: baseline.statuses,
      statusAcceptance: baseline.statusAcceptance,
      cases: baseline.cases,
    };
    for (const item of successful.slice(1)) {
      const witness = JSON.parse(await readFile(path.join(root, item.result), 'utf8'));
      compare(
        `cross-runtime semantics against ${successful[0].authority}/${successful[0].language}`,
        {
          model: witness.model,
          wireFields: sortedStrings(witness.wireFields),
          requiredFields: sortedStrings(witness.requiredFields),
          optionalFields: sortedStrings(witness.optionalFields),
          statuses: witness.statuses,
          statusAcceptance: witness.statusAcceptance,
          cases: witness.cases,
        },
        baselineSemantics,
        'cross-runtime-or-authority-witness-mismatch',
        `${item.authority}/${item.language}`,
      );
    }
  }

  return fixture;
}

let fixture = null;
let fatal = null;
try {
  fixture = await main();
} catch (error) {
  fatal = error;
  finding('generated-runtime-gate-failure', error.stack ?? error.message);
}

let harnesses = {};
try {
  harnesses = await harnessDigests();
} catch (error) {
  finding('harness-digest-failure', error.message);
}

const sourceDigests = {};
for (const relative of [
  'contracts/persistence/idempotency-record.tsp',
  'contracts/persistence/idempotency-record.schema.json',
  'fixtures/generated-runtime-conformance.json',
  'scripts/generated_runtime_matrix.mjs',
]) {
  sourceDigests[relative] = await requiredFileDigest(path.join(root, relative));
}

const failedCells = witnesses.filter((item) => item.state === 'failed').length;
const semanticFindings = discrepancies.some((item) =>
  [
    'wire-field-mismatch',
    'required-field-mismatch',
    'optional-field-mismatch',
    'status-vocabulary-mismatch',
    'status-runtime-mismatch',
    'runtime-case-set-mismatch',
    'runtime-acceptance-mismatch',
    'runtime-roundtrip-mismatch',
    'cross-runtime-or-authority-witness-mismatch',
  ].includes(item.kind),
);
const status =
  failedCells > 0 || fatal
    ? 'failed'
    : semanticFindings || discrepancies.length > 0
      ? 'stopped_for_evaluation'
      : witnesses.length === lanes.length * languages.length
        ? 'passed'
        : 'partial';

const receipt = {
  schema: 'ores.generated-runtime-convergence-report/v1',
  repository: 'ORESoftware/ores-middleware',
  startedAt,
  endedAt: new Date().toISOString(),
  actor: process.env.GITHUB_ACTOR ?? process.env.USER ?? 'unknown',
  commit: process.env.GITHUB_SHA ?? null,
  authorities: lanes,
  languages,
  fixture: fixture
    ? {
        path: path.relative(root, fixturePath),
        schema: fixture.schema,
        authority: fixture.authority,
        cases: fixture.cases.length,
        sha256: await sha256File(fixturePath),
      }
    : null,
  sourceDigests,
  harnessDigests: harnesses,
  witnesses,
  checks,
  discrepancies,
  status,
  zeroUnexplainedFindings: status === 'passed',
};
await writeJson(receiptPath, receipt);
console.log(`generated runtime convergence status=${status} receipt=${path.relative(root, receiptPath)}`);
if (status === 'failed') process.exitCode = 1;
else if (status === 'stopped_for_evaluation' || status === 'partial') process.exitCode = 2;
