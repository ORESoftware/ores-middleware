#!/usr/bin/env node

import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";

const root = path.resolve(process.argv[2] ?? process.cwd());
const outputRoot = path.resolve(
  process.argv[3] ?? path.join(root, "target", "js-runtime-surface-matrix"),
);
const startedAt = new Date().toISOString();

const runtimes = [
  { name: "node", command: [process.execPath] },
  { name: "bun", command: [process.env.ORES_BUN_BIN ?? "bun"] },
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
  },
];

const commonProbes = [
  "runtime-metadata-smoke.mjs",
  "runtime-version-floor-smoke.mjs",
  "package-metadata-consistency-smoke.mjs",
  "generic-context-smoke.mjs",
  "adversarial-boundary-smoke.mjs",
  "abort-signal-smoke.mjs",
  "web-streams-smoke.mjs",
  "url-semantics-smoke.mjs",
  "headers-semantics-smoke.mjs",
  "request-clone-smoke.mjs",
  "response-clone-smoke.mjs",
  "crypto-uuid-smoke.mjs",
  "timers-smoke.mjs",
  "text-encoding-smoke.mjs",
  "formdata-smoke.mjs",
  "blob-smoke.mjs",
  "json-smoke.mjs",
  "promise-order-smoke.mjs",
  "error-cause-smoke.mjs",
  "structured-clone-smoke.mjs",
  "webcrypto-digest-smoke.mjs",
  "arraybuffer-smoke.mjs",
  "object-freeze-smoke.mjs",
  "map-set-smoke.mjs",
  "reflect-smoke.mjs",
  "searchparams-smoke.mjs",
  "weakmap-smoke.mjs",
  "regexp-smoke.mjs",
];

const runtimeSpecific = {
  node: ["node-fetch-primitives-smoke.mjs"],
  bun: ["bun-fetch-primitives-smoke.mjs"],
  deno: ["deno-fetch-primitives-smoke.mjs"],
};

const packagedProbes = [
  "packaged-generic-context-smoke.mjs",
  "packaged-adversarial-boundary-smoke.mjs",
];

const checks = [];
const discrepancies = [];
const witnesses = [];

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

function finding(kind, detail, runtime, probe) {
  discrepancies.push({
    kind,
    detail,
    runtime,
    probe,
    fingerprint: sha256(`${kind}\0${runtime}\0${probe}\0${detail}`),
  });
}

async function run(runtime, probe, extraArguments = []) {
  const probePath = path.join(root, "tests", "runtime-portability", probe);
  const id = `${runtime.name}/${probe.replace(/\.mjs$/u, "")}`;
  const logBase = path.join(outputRoot, "logs", id);
  await ensureDirectory(path.dirname(logBase));
  const command = [...runtime.command, probePath, ...extraArguments];
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
  child.stdout.on("data", (chunk) => { stdout += chunk; });
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  const exitCode = await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", resolve);
  });
  await writeFile(`${logBase}.stdout.log`, stdout, "utf8");
  await writeFile(`${logBase}.stderr.log`, stderr, "utf8");
  checks.push({
    id,
    runtime: runtime.name,
    probe,
    command,
    exitCode,
    durationMs: Date.now() - started,
    stdoutLog: path.relative(root, `${logBase}.stdout.log`),
    stderrLog: path.relative(root, `${logBase}.stderr.log`),
  });
  if (exitCode !== 0) throw new Error(`${id} exited ${exitCode}`);
  const lines = stdout
    .split(/\r?\n/u)
    .map((line) => line.trim())
    .filter((line) => line.startsWith("{") && line.endsWith("}"));
  if (lines.length === 0) throw new Error(`${id} emitted no JSON witness`);
  return JSON.parse(lines.at(-1));
}

await rm(outputRoot, { recursive: true, force: true });
await ensureDirectory(outputRoot);

for (const runtime of runtimes) {
  for (const probe of [...commonProbes, ...runtimeSpecific[runtime.name], ...packagedProbes]) {
    const before = discrepancies.length;
    try {
      const witness = await run(runtime, probe);
      if (
        witness.schema !== "ores.middleware.js-runtime-portability-smoke/v1" ||
        !["passed", "skipped"].includes(witness.status)
      ) {
        finding("invalid-witness", stable(witness), runtime.name, probe);
      }
      if (witness.status === "skipped") {
        finding("unexpected-skip", stable(witness), runtime.name, probe);
      }
      witnesses.push({
        runtime: runtime.name,
        probe,
        suite: witness.suite ?? null,
        state: discrepancies.length === before ? "executed" : "discrepant",
        witness,
      });
    } catch (error) {
      finding("probe-failure", error.message, runtime.name, probe);
      witnesses.push({ runtime: runtime.name, probe, state: "failed", error: error.message });
    }
  }
}

// The generic/context and adversarial probes are intended to have the same
// semantic shape on every runtime. Runtime-specific identity/version strings
// remain outside these normalized comparisons.
for (const probe of [
  "generic-context-smoke.mjs",
  "adversarial-boundary-smoke.mjs",
  "packaged-generic-context-smoke.mjs",
  "packaged-adversarial-boundary-smoke.mjs",
]) {
  const group = witnesses.filter((item) => item.probe === probe && item.witness);
  if (group.length !== runtimes.length) {
    finding("incomplete-cross-runtime-witness", `count=${group.length}`, "all", probe);
    continue;
  }
  const normalize = (witness) => {
    const { runtime: _runtime, ...rest } = witness;
    return rest;
  };
  const baseline = stable(normalize(group[0].witness));
  for (const item of group.slice(1)) {
    if (stable(normalize(item.witness)) !== baseline) {
      finding(
        "cross-runtime-semantic-mismatch",
        `actual=${stable(normalize(item.witness))} expected=${baseline}`,
        item.runtime,
        probe,
      );
    }
  }
}

// Deno's generic core must not require filesystem, network, env, or sys
// permissions. Run this one separately with no permission grants at all.
try {
  const deno = { name: "deno-no-permissions", command: [process.env.ORES_DENO_BIN ?? "deno", "run", "--no-prompt"] };
  const witness = await run(deno, "deno-permissions-smoke.mjs");
  if (witness.status !== "passed") {
    finding("deno-permission-boundary-mismatch", stable(witness), deno.name, "deno-permissions-smoke.mjs");
  }
  witnesses.push({ runtime: deno.name, probe: "deno-permissions-smoke.mjs", state: "executed", witness });
} catch (error) {
  finding("deno-permission-boundary-failure", error.message, "deno-no-permissions", "deno-permissions-smoke.mjs");
}

const expectedPerRuntime = commonProbes.length + packagedProbes.length + 1;
const passed =
  discrepancies.length === 0 &&
  runtimes.every((runtime) =>
    witnesses.filter((item) => item.runtime === runtime.name && item.state === "executed").length === expectedPerRuntime,
  ) &&
  witnesses.some((item) => item.runtime === "deno-no-permissions" && item.state === "executed");

const sourceDigests = {};
for (const probe of new Set([...commonProbes, ...Object.values(runtimeSpecific).flat(), ...packagedProbes, "deno-permissions-smoke.mjs"])) {
  const filename = path.join(root, "tests", "runtime-portability", probe);
  sourceDigests[path.relative(root, filename)] = await sha256File(filename);
}
for (const filename of [
  "src/ts/dist/generic.js",
  "src/ts/dist/context.js",
  "target/ts/dist/generic.js",
  "target/ts/dist/context.js",
]) {
  sourceDigests[filename] = await sha256File(path.join(root, filename));
}

const receipt = {
  schema: "ores.middleware.js-runtime-surface-matrix/v1",
  repository: "ORESoftware/ores-middleware",
  commit: process.env.ORES_WORKFLOW_SOURCE_SHA ?? process.env.GITHUB_SHA ?? null,
  startedAt,
  endedAt: new Date().toISOString(),
  runtimes: runtimes.map((runtime) => runtime.name),
  sourceDigests,
  expectedPerRuntime,
  witnesses,
  checks,
  discrepancies,
  status: passed ? "passed" : "failed",
  zeroUnexplainedFindings: passed,
};

const receiptPath = path.join(outputRoot, "receipt.json");
await writeFile(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`, "utf8");
console.log(`js runtime surface matrix status=${receipt.status} receipt=${path.relative(root, receiptPath)}`);
if (!passed) process.exitCode = 1;
