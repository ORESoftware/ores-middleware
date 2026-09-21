#!/usr/bin/env node
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const languageFlag = args.indexOf("--language");
if (languageFlag < 0 || !args[languageFlag + 1]) {
  throw new Error("usage: node scripts/check-runtime-descriptor.mjs --language <id> <descriptor.json>");
}
const language = args[languageFlag + 1];
const descriptorPath = args.find((arg, index) => !arg.startsWith("--") && index !== languageFlag + 1);
if (!descriptorPath) {
  throw new Error("descriptor JSON path is required");
}

const readJson = async (path) => JSON.parse(await readFile(resolve(root, path), "utf8"));
const governance = await readJson("governance/polyglot-participants.v1.json");
const descriptorSchema = await readJson("contracts/json-schema/adapter-descriptor.schema.json");
const stackSchema = await readJson("contracts/json-schema/middleware-stack.schema.json");
const descriptor = await readJson(descriptorPath);

const participant = governance.participants.find((entry) => entry.id === language);
assert(participant, `ungoverned descriptor language: ${language}`);
const adapterManifest = await readJson(participant.adapterManifest);

const descriptorDef = descriptorSchema.$defs?.adapterDescriptor;
const symbolsDef = descriptorSchema.$defs?.sdkOperationSymbols;
assert(descriptorDef && symbolsDef, "adapter descriptor contract definitions are missing");
const allowedLanguages = descriptorDef.properties.language.anyOf.map((entry) => entry.const).sort();
const expectedCapabilities = [...(stackSchema.$defs?.capability?.enum ?? [])].sort();
const expectedFields = [...descriptorDef.required].sort();
const expectedSymbols = [...symbolsDef.required].sort();
assert(expectedCapabilities.length > 0, "middleware capability contract is empty");

const uniqueSortedStrings = (values, label) => {
  assert(Array.isArray(values) && values.length > 0, `${label} must be a non-empty array`);
  for (const value of values) assert.equal(typeof value, "string", `${label} values must be strings`);
  assert.equal(new Set(values).size, values.length, `${label} must not contain duplicates`);
  return [...values].sort();
};

assert.deepEqual(Object.keys(descriptor).sort(), expectedFields, `${language}: descriptor fields drifted from contract`);
assert.equal(descriptor.contractVersion, descriptorDef.properties.contractVersion.const, `${language}: contractVersion mismatch`);
assert(allowedLanguages.includes(descriptor.language), `${language}: language is not admitted by descriptor contract`);
assert.equal(descriptor.language, language, `${language}: emitted descriptor language mismatch`);
assert.equal(descriptor.runtime, adapterManifest.runtime, `${language}: runtime drifted from adapter manifest`);
assert.equal(descriptor.packageName, adapterManifest.packageName, `${language}: packageName drifted from adapter manifest`);
assert.deepEqual(
  uniqueSortedStrings(descriptor.frameworkAdapters, `${language}.frameworkAdapters`),
  uniqueSortedStrings(adapterManifest.frameworkAdapters, `${language}.adapterManifest.frameworkAdapters`),
  `${language}: framework adapter surface drifted from governed manifest`,
);
assert.deepEqual(
  uniqueSortedStrings(descriptor.capabilities, `${language}.capabilities`),
  expectedCapabilities,
  `${language}: runtime must expose the complete portable capability contract`,
);
assert(descriptor.operationSymbols && typeof descriptor.operationSymbols === "object" && !Array.isArray(descriptor.operationSymbols), `${language}: operationSymbols must be an object`);
assert.deepEqual(Object.keys(descriptor.operationSymbols).sort(), expectedSymbols, `${language}: operation symbol categories drifted from contract`);
assert.deepEqual(Object.keys(adapterManifest.operationSymbols).sort(), expectedSymbols, `${language}: adapter manifest symbol categories drifted from contract`);
for (const key of expectedSymbols) {
  assert.equal(typeof descriptor.operationSymbols[key], "string", `${language}: ${key} runtime symbol must be a string`);
  assert(descriptor.operationSymbols[key].length > 0, `${language}: ${key} runtime symbol must be non-empty`);
  assert.equal(descriptor.operationSymbols[key], adapterManifest.operationSymbols[key], `${language}: ${key} runtime symbol drifted from adapter manifest`);
}

console.log(JSON.stringify({
  schema: "ores.middleware.runtime-descriptor-check/v1",
  language,
  contractVersion: descriptor.contractVersion,
  runtime: descriptor.runtime,
  capabilityCount: descriptor.capabilities.length,
  operationCount: expectedSymbols.length,
  status: "pass"
}, null, 2));
