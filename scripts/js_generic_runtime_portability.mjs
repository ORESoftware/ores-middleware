#!/usr/bin/env node

import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";

const startedAt = new Date().toISOString();
const root = path.resolve(process.argv[2] ?? process.cwd());
const outputRoot = path.resolve(
  process.argv[3] ?? path.join(root, "target", "js-generic-runtime-portability"),
);
const smokePath = path.join(root, "tests", "runtime-portability", "generic-context-smoke.mjs");
const surfaces = [
  { name: "source", dist: path.join(root, "src", "ts", "dist") },
  { name: "packaged", dist: path.join(root, "target", "ts", "dist") },
];
const runtimes = [
  {
    name: "node",
    command: [process.execPath],
    versionCommand: [process.execPath, "--version"],
  },
  {
    name: "bun",
    command: [process.env.ORES_BUN_BIN ?? "bun"],
    versionCommand: [process.env.ORES_BUN_BIN ?? "bun", "--version"],
  },
  {
    name: "deno",
    command: [
      process.env.ORES_DENO_BIN ?? "deno",
      "run",
      "--no-prompt",
      "--allow-read",
      "--allow-env",
      "--allow-sys",
      "--node-modules-dir=manual",
    ],
    versionCommand: [process.env.ORES_DENO_BIN ?? "deno", "--version"],
  },
];

const checks = [];
const runtimeWitnesses = [];
const discrepancies = [];
const runtimeVersions = {};

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") {
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
  return createHash("sha256").update(value).digest("hex");
}

async function sha256File(filename) {
  return sha256(await readFile(filename));
}

async function ensureDirectory(directory) {
  await mkdir(directory, { recursive: true });
}

async function writeJson(filename, value) {
  await ensureDirectory(path.dirname(filename));
  await writeFile(filename, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

function finding(kind, detail, cell = null) {
  discrepancies.push({
    kind,
    detail,
    cell,
    fingerprint: sha256(`${kind}\0${cell ?? ""}\0${detail}`),
  });
}

async function runCommand(id, command) {
  const logBase = path.join(outputRoot, "logs", id);
  await ensureDirectory(path.dirname(logBase));
  const started = Date.now();
  const child = spawn(command[0], command.slice(1), {
    cwd: root,
    env: { ...process.env, CI: "true", NO_COLOR: "1" },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  child.stdout.on("data", (chunk) => {
    stdout += chunk;
  });
  child.stderr.on("data", (chunk) => {
    stderr += chunk;
  });
  const exitCode = await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", resolve);
  });
  await writeFile(`${logBase}.stdout.log`, stdout, "utf8");
  await writeFile(`${logBase}.stderr.log`, stderr, "utf8");
  checks.push({
    id,
    command,
    exitCode,
    durationMs: Date.now() - started,
    stdoutLog: path.relative(root, `${logBase}.stdout.log`),
    stderrLog: path.relative(root, `${logBase}.stderr.log`),
  });
  if (exitCode !== 0) {
    throw new Error(`${id} failed with exit code ${exitCode}: ${command.join(" ")}`);
  }
  return { stdout, stderr };
}

function parseLastJsonLine(stdout, cell) {
  const candidates = stdout
    .split(/\r?\n/u)
    .map((line) => line.trim())
    .filter((line) => line.startsWith("{") && line.endsWith("}"));
  if (candidates.length === 0) throw new Error(`${cell} emitted no JSON result`);
  return JSON.parse(candidates.at(-1));
}

function normalizedWitness(witness) {
  return {
    schema: witness.schema,
    suite: witness.suite,
    status: witness.status,
    checks: witness.checks,
    concurrentContexts: witness.concurrentContexts,
  };
}

await rm(outputRoot, { recursive: true, force: true });
await ensureDirectory(outputRoot);

for (const runtime of runtimes) {
  try {
    const version = await runCommand(`versions/${runtime.name}`, runtime.versionCommand);
    runtimeVersions[runtime.name] = version.stdout.trim().split(/\r?\n/u)[0];
  } catch (error) {
    runtimeVersions[runtime.name] = null;
    finding("runtime-version-failure", error.message, runtime.name);
  }
}

let baseline = null;
for (const surface of surfaces) {
  const distUrl = pathToFileURL(`${surface.dist}${path.sep}`).href;
  for (const runtime of runtimes) {
    const cell = `${surface.name}/${runtime.name}`;
    try {
      const result = await runCommand(
        `generic-context/${surface.name}/${runtime.name}`,
        [...runtime.command, smokePath, distUrl],
      );
      const witness = parseLastJsonLine(result.stdout, cell);
      const normalized = normalizedWitness(witness);
      const before = discrepancies.length;

      if (
        witness.schema !== "ores.middleware.js-runtime-portability-smoke/v1" ||
        witness.suite !== "generic-context" ||
        witness.status !== "passed" ||
        !Number.isSafeInteger(witness.checks) ||
        witness.checks < 11 ||
        witness.concurrentContexts !== 32
      ) {
        finding("generic-context-smoke-mismatch", stable(witness), cell);
      }

      if (surface.name === "source" && runtime.name === "node") {
        baseline = normalized;
      } else if (!baseline || stable(normalized) !== stable(baseline)) {
        finding(
          "generic-context-runtime-semantic-mismatch",
          `actual=${stable(normalized)} expected=${stable(baseline)}`,
          cell,
        );
      }

      const resultPath = path.join(outputRoot, "results", surface.name, `${runtime.name}.json`);
      await writeJson(resultPath, witness);
      runtimeWitnesses.push({
        surface: surface.name,
        runtime: runtime.name,
        state: discrepancies.length === before ? "executed" : "discrepant",
        result: path.relative(root, resultPath),
        resultSha256: await sha256File(resultPath),
      });
    } catch (error) {
      finding("generic-context-runtime-failure", error.message, cell);
      runtimeWitnesses.push({
        surface: surface.name,
        runtime: runtime.name,
        state: "failed",
        error: error.message,
      });
    }
  }
}

const expectedWitnessCount = surfaces.length * runtimes.length;
const passed =
  discrepancies.length === 0 &&
  runtimeWitnesses.length === expectedWitnessCount &&
  runtimeWitnesses.every((item) => item.state === "executed");

const sourceDigests = {
  [path.relative(root, smokePath)]: await sha256File(smokePath),
};
for (const surface of surfaces) {
  sourceDigests[`${surface.name}/generic.js`] = await sha256File(path.join(surface.dist, "generic.js"));
  sourceDigests[`${surface.name}/context.js`] = await sha256File(path.join(surface.dist, "context.js"));
  sourceDigests[`${surface.name}/index.js`] = await sha256File(path.join(surface.dist, "index.js"));
}

const receipt = {
  schema: "ores.middleware.js-generic-runtime-portability-report/v1",
  repository: "ORESoftware/ores-middleware",
  commit: process.env.ORES_WORKFLOW_SOURCE_SHA ?? process.env.GITHUB_SHA ?? null,
  startedAt,
  endedAt: new Date().toISOString(),
  language: "typescript",
  surface: ["generic", "context", "descriptor", "packaged-closure"],
  runtimes: runtimes.map((runtime) => ({
    name: runtime.name,
    version: runtimeVersions[runtime.name] ?? null,
  })),
  packageSurfaces: surfaces.map((surface) => surface.name),
  sourceDigests,
  runtimeWitnesses,
  checks,
  discrepancies,
  status: passed ? "passed" : "failed",
  zeroUnexplainedFindings: passed,
};

const receiptPath = path.join(outputRoot, "receipt.json");
await writeJson(receiptPath, receipt);
console.log(
  `js generic runtime portability status=${receipt.status} receipt=${path.relative(root, receiptPath)}`,
);
if (!passed) process.exitCode = 1;
