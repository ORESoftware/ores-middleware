#!/usr/bin/env node
import assert from "node:assert/strict";
import { lstat, readFile, readdir } from "node:fs/promises";
import { dirname, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..");
const registryPath = "governance/polyglot-participants.v1.json";

function fail(message) {
  throw new Error(message);
}

function normalizeRepoPath(value, label) {
  if (typeof value !== "string" || value.length === 0 || value.includes("\0")) {
    fail(`${label} must be a non-empty repository-relative path`);
  }
  const resolved = resolve(root, value);
  const rel = relative(root, resolved);
  if (rel === ".." || rel.startsWith(`..${sep}`)) fail(`${label} escapes repository root: ${value}`);
  return rel.split(sep).join("/");
}

async function readJson(path, label) {
  const normalized = normalizeRepoPath(path, label);
  const stat = await lstat(resolve(root, normalized));
  if (!stat.isFile() || stat.isSymbolicLink()) fail(`${label} must be a real file: ${normalized}`);
  return JSON.parse(await readFile(resolve(root, normalized), "utf8"));
}

function sorted(values) {
  return [...values].sort((a, b) => String(a).localeCompare(String(b)));
}

function workflowJobBlock(workflow, jobName) {
  const lines = workflow.split(/\r?\n/);
  const marker = `  ${jobName}:`;
  const start = lines.findIndex((line) => line === marker);
  if (start < 0) fail(`missing contract-conformance CI job: ${jobName}`);
  let end = lines.length;
  for (let index = start + 1; index < lines.length; index += 1) {
    if (/^  [A-Za-z0-9_-]+:$/.test(lines[index])) {
      end = index;
      break;
    }
  }
  return lines.slice(start, end).join("\n");
}

function zpkgTargetDir(zpkg, target) {
  const lines = zpkg.split(/\r?\n/);
  const marker = `[targets.${target}]`;
  const start = lines.findIndex((line) => line.trim() === marker);
  if (start < 0) fail(`missing .zpkg.toml target: ${target}`);
  let end = lines.length;
  for (let index = start + 1; index < lines.length; index += 1) {
    if (/^\s*\[.+\]\s*$/.test(lines[index])) {
      end = index;
      break;
    }
  }
  for (const line of lines.slice(start + 1, end)) {
    const match = /^\s*dir\s*=\s*"([^"]+)"\s*$/.exec(line);
    if (match) return match[1];
  }
  fail(`missing dir in .zpkg.toml target: ${target}`);
}

const registry = await readJson(registryPath, "polyglot governance registry");
assert.equal(registry.schema, "ores.middleware.polyglot-governance/v1", "unexpected governance schema");
assert.equal(registry.repository, "ORESoftware/ores-middleware", "unexpected governance repository");
for (const key of [
  "allSourceDirectoriesMustBeGoverned",
  "conformanceParticipantsMustMatchGovernedLanguages",
  "adapterOperationSurfaceMustMatch",
  "zpkgTargetsMustMatchGovernedLanguages",
  "contractConformanceCiMustExerciseEveryLanguage",
  "missingOrExtraParticipantFailsClosed",
  "scaffoldCoverageMustDeclareDelegatedBehavioralAuthority",
]) {
  assert.equal(registry.policy?.[key], true, `policy.${key} must be true`);
}
assert.equal(registry.policy?.generatedArtifactsAreAuthority, false);
assert.equal(registry.policy?.runtimeSpecificGoldensAllowed, false);

const sourceRoot = normalizeRepoPath(registry.sourceRoot, "sourceRoot");
const contractRoot = normalizeRepoPath(registry.contractRoot, "contractRoot");
assert.equal(sourceRoot, "src");
assert.equal(contractRoot, "contracts");

for (const path of [sourceRoot, contractRoot, "conformance", "governance"]) {
  const stat = await lstat(resolve(root, path));
  if (!stat.isDirectory() || stat.isSymbolicLink()) fail(`${path}/ must be a real directory`);
}

if (!Array.isArray(registry.participants) || registry.participants.length === 0) fail("participants must be non-empty");
if (!Array.isArray(registry.requiredOperationSymbols) || registry.requiredOperationSymbols.length === 0) {
  fail("requiredOperationSymbols must be non-empty");
}
if (!Array.isArray(registry.ignoredSourceDirectories)) fail("ignoredSourceDirectories must be an array");

const participantIds = new Set();
const participantDirs = new Set();
const zpkgTargets = new Set();
for (const participant of registry.participants) {
  if (!participant || typeof participant !== "object" || Array.isArray(participant)) fail("participant must be an object");
  if (typeof participant.id !== "string" || !/^[a-z0-9][a-z0-9_-]*$/.test(participant.id)) {
    fail(`invalid participant id: ${participant?.id}`);
  }
  if (participantIds.has(participant.id)) fail(`duplicate participant id: ${participant.id}`);
  participantIds.add(participant.id);

  const sourceDir = normalizeRepoPath(participant.sourceDir, `${participant.id}.sourceDir`);
  const adapterManifestPath = normalizeRepoPath(participant.adapterManifest, `${participant.id}.adapterManifest`);
  if (!sourceDir.startsWith("src/") || sourceDir.split("/").length !== 2) {
    fail(`${participant.id}.sourceDir must be an immediate src/ child`);
  }
  if (participantDirs.has(sourceDir)) fail(`duplicate governed source directory: ${sourceDir}`);
  participantDirs.add(sourceDir);
  if (adapterManifestPath !== `${sourceDir}/adapter.manifest.json`) {
    fail(`${participant.id}.adapterManifest must be ${sourceDir}/adapter.manifest.json`);
  }

  const adapter = await readJson(adapterManifestPath, `${participant.id} adapter manifest`);
  assert.equal(adapter.language, participant.id, `${participant.id} adapter language mismatch`);
  if (typeof adapter.runtime !== "string" || adapter.runtime.length === 0) fail(`${participant.id} runtime is missing`);
  if (typeof adapter.packageName !== "string" || adapter.packageName.length === 0) fail(`${participant.id} packageName is missing`);
  if (!Array.isArray(adapter.frameworkAdapters) || adapter.frameworkAdapters.length === 0) {
    fail(`${participant.id} frameworkAdapters must be non-empty`);
  }
  if (!adapter.operationSymbols || typeof adapter.operationSymbols !== "object" || Array.isArray(adapter.operationSymbols)) {
    fail(`${participant.id} operationSymbols must be an object`);
  }
  assert.deepEqual(
    sorted(Object.keys(adapter.operationSymbols)),
    sorted(registry.requiredOperationSymbols),
    `${participant.id} semantic operation surface drifted`,
  );
  for (const symbol of registry.requiredOperationSymbols) {
    if (typeof adapter.operationSymbols[symbol] !== "string" || adapter.operationSymbols[symbol].length === 0) {
      fail(`${participant.id} missing operation symbol binding: ${symbol}`);
    }
  }

  if (typeof participant.zpkgTarget !== "string" || participant.zpkgTarget.length === 0) fail(`${participant.id} zpkgTarget is missing`);
  if (zpkgTargets.has(participant.zpkgTarget)) fail(`duplicate zpkg target: ${participant.zpkgTarget}`);
  zpkgTargets.add(participant.zpkgTarget);
  if (typeof participant.ciJob !== "string" || participant.ciJob.length === 0) fail(`${participant.id} ciJob is missing`);
  if (!Array.isArray(participant.requiredCiTokens) || participant.requiredCiTokens.length < 2) {
    fail(`${participant.id} must declare runtime-test and contract-check CI tokens`);
  }
}

const ignoredNames = new Set();
for (const entry of registry.ignoredSourceDirectories) {
  if (!entry || typeof entry !== "object" || typeof entry.name !== "string" || typeof entry.reason !== "string" || entry.reason.trim().length === 0) {
    fail("ignoredSourceDirectories entries require name and non-empty reason");
  }
  if (ignoredNames.has(entry.name)) fail(`duplicate ignored source directory: ${entry.name}`);
  ignoredNames.add(entry.name);
}

const sourceEntries = await readdir(resolve(root, sourceRoot), { withFileTypes: true });
const discoveredDirs = [];
for (const entry of sourceEntries) {
  const path = resolve(root, sourceRoot, entry.name);
  const stat = await lstat(path);
  if (stat.isSymbolicLink()) fail(`symbolic links are forbidden in src/: src/${entry.name}`);
  if (stat.isDirectory()) discoveredDirs.push(`src/${entry.name}`);
}
const governedOrIgnored = new Set([
  ...participantDirs,
  ...[...ignoredNames].map((name) => `src/${name}`),
]);
assert.deepEqual(
  sorted(discoveredDirs),
  sorted(governedOrIgnored),
  "every immediate src/ directory must be governed or explicitly ignored",
);

const conformance = await readJson(registry.conformanceManifest, "conformance manifest");
assert.equal(conformance.repository, registry.repository, "conformance repository mismatch");
assert.equal(conformance.governanceRegistry, registryPath, "conformance manifest must bind the governance registry");
const contractRoots = new Set((conformance.contractRoots ?? []).map((value) => String(value).replace(/\/$/, "")));
if (!contractRoots.has(contractRoot)) fail(`conformance manifest must bind contract root ${contractRoot}/`);
if (!Array.isArray(conformance.requiredParticipants)) fail("conformance requiredParticipants must be an array");
const conformanceIds = conformance.requiredParticipants.map((entry) => typeof entry === "string" ? entry : entry?.id);
if (conformanceIds.some((id) => typeof id !== "string" || id.length === 0)) fail("conformance participant ids must be non-empty strings");
assert.deepEqual(
  sorted(conformanceIds),
  sorted(participantIds),
  "conformance participants must exactly match governed src languages",
);

const zpkgPath = normalizeRepoPath(registry.zpkgManifest, "zpkgManifest");
const zpkg = await readFile(resolve(root, zpkgPath), "utf8");
for (const participant of registry.participants) {
  assert.equal(
    zpkgTargetDir(zpkg, participant.zpkgTarget),
    participant.sourceDir,
    `${participant.id} Zed target points at the wrong source directory`,
  );
}

const workflowPath = normalizeRepoPath(registry.contractConformanceWorkflow, "contractConformanceWorkflow");
const workflow = await readFile(resolve(root, workflowPath), "utf8");
for (const participant of registry.participants) {
  const block = workflowJobBlock(workflow, participant.ciJob);
  for (const token of participant.requiredCiTokens) {
    if (!block.includes(token)) {
      fail(`${participant.id} contract-conformance job ${participant.ciJob} is missing required token: ${token}`);
    }
  }
}

if (conformance.coverage?.status === "scaffold-only") {
  const delegated = registry.delegatedBehavioralConformance;
  if (!delegated || delegated.mode !== "workflow" || delegated.failClosed !== true) {
    fail("scaffold-only coverage requires an explicit fail-closed delegated behavioral workflow");
  }
  const delegatedWorkflow = normalizeRepoPath(delegated.workflow, "delegatedBehavioralConformance.workflow");
  assert.equal(delegatedWorkflow, workflowPath, "delegated behavioral workflow must be the governed contract-conformance workflow");
  assert.equal(conformance.coverage.authorityMode, "delegated-workflow", "scaffold coverage must declare delegated-workflow authority");
  assert.equal(conformance.coverage.authorityWorkflow, delegated.workflow, "conformance manifest authority workflow drifted from governance");
  if (typeof delegated.reason !== "string" || delegated.reason.trim().length === 0) {
    fail("delegated behavioral conformance requires a non-empty reason");
  }
}

console.log(JSON.stringify({
  schema: "ores.middleware.polyglot-governance.check/v1",
  repository: registry.repository,
  contractRoot,
  conformanceManifest: registry.conformanceManifest,
  governedParticipants: sorted(participantIds),
  governedSourceDirectories: sorted(participantDirs),
  ignoredSourceDirectories: sorted(ignoredNames),
  status: "pass"
}, null, 2));
