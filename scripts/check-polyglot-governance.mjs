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
  return [...values].sort((a, b) => a.localeCompare(b));
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function workflowJobBlock(workflow, jobName) {
  const marker = new RegExp(`^  ${escapeRegExp(jobName)}:\\s*$`, "m");
  const match = marker.exec(workflow);
  if (!match) fail(`missing contract-conformance CI job: ${jobName}`);
  const start = match.index;
  const remainder = workflow.slice(start + match[0].length);
  const next = /^  [A-Za-z0-9_-]+:\s*$/m.exec(remainder);
  const end = next ? start + match[0].length + next.index : workflow.length;
  return workflow.slice(start, end);
}

function zpkgTargetDir(zpkg, target) {
  const section = new RegExp(`^\\[targets\\.${escapeRegExp(target)}\\]\\s*$([\\s\\S]*?)(?=^\\[|\\Z)`, "m").exec(zpkg);
  if (!section) fail(`missing .zpkg.toml target: ${target}`);
  const dir = /^dir\s*=\s*"([^"]+)"\s*$/m.exec(section[1]);
  if (!dir) fail(`missing dir in .zpkg.toml target: ${target}`);
  return dir[1];
}

const registry = await readJson(registryPath, "polyglot governance registry");
assert.equal(registry.schema, "ores.middleware.polyglot-governance/v1", "unexpected governance schema");
assert.equal(registry.repository, "ORESoftware/ores-middleware", "unexpected governance repository");
assert.equal(registry.policy?.allSourceDirectoriesMustBeGoverned, true);
assert.equal(registry.policy?.conformanceParticipantsMustMatchGovernedLanguages, true);
assert.equal(registry.policy?.adapterOperationSurfaceMustMatch, true);
assert.equal(registry.policy?.zpkgTargetsMustMatchGovernedLanguages, true);
assert.equal(registry.policy?.contractConformanceCiMustExerciseEveryLanguage, true);
assert.equal(registry.policy?.generatedArtifactsAreAuthority, false);
assert.equal(registry.policy?.runtimeSpecificGoldensAllowed, false);
assert.equal(registry.policy?.missingOrExtraParticipantFailsClosed, true);

const sourceRoot = normalizeRepoPath(registry.sourceRoot, "sourceRoot");
const contractRoot = normalizeRepoPath(registry.contractRoot, "contractRoot");
assert.equal(sourceRoot, "src");
assert.equal(contractRoot, "contracts");

const sourceStat = await lstat(resolve(root, sourceRoot));
const contractStat = await lstat(resolve(root, contractRoot));
if (!sourceStat.isDirectory() || sourceStat.isSymbolicLink()) fail("src/ must be a real directory");
if (!contractStat.isDirectory() || contractStat.isSymbolicLink()) fail("contracts/ must be a real directory");

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
assert.deepEqual(sorted(discoveredDirs), sorted(governedOrIgnored), "every immediate src/ directory must be governed or explicitly ignored");

const conformance = await readJson(registry.conformanceManifest, "conformance manifest");
assert.equal(conformance.repository, registry.repository, "conformance repository mismatch");
const contractRoots = new Set((conformance.contractRoots ?? []).map((value) => String(value).replace(/\/$/, "")));
if (!contractRoots.has(contractRoot)) fail(`conformance manifest must bind contract root ${contractRoot}/`);
if (!Array.isArray(conformance.requiredParticipants)) fail("conformance requiredParticipants must be an array");
const conformanceIds = conformance.requiredParticipants.map((entry) => typeof entry === "string" ? entry : entry?.id);
assert.deepEqual(sorted(conformanceIds), sorted(participantIds), "conformance participants must exactly match governed src languages");

const zpkg = await readFile(resolve(root, normalizeRepoPath(registry.zpkgManifest, "zpkgManifest")), "utf8");
for (const participant of registry.participants) {
  assert.equal(zpkgTargetDir(zpkg, participant.zpkgTarget), participant.sourceDir, `${participant.id} Zed target points at the wrong source directory`);
}

const workflow = await readFile(resolve(root, normalizeRepoPath(registry.contractConformanceWorkflow, "contractConformanceWorkflow")), "utf8");
for (const participant of registry.participants) {
  const block = workflowJobBlock(workflow, participant.ciJob);
  for (const token of participant.requiredCiTokens) {
    if (!block.includes(token)) fail(`${participant.id} contract-conformance job ${participant.ciJob} is missing required token: ${token}`);
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
