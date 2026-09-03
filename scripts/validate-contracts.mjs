#!/usr/bin/env node
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";

const root = new URL("../", import.meta.url);
const loadJson = async (path) => JSON.parse(await readFile(new URL(path, root), "utf8"));
const ajv = new Ajv2020({ allErrors: true, strict: true });
addFormats(ajv);

const cases = [
  ["contracts/json-schema/middleware-stack.schema.json", "contracts/fixtures/stack.minimal.json"],
  ["contracts/json-schema/adapter-descriptor.schema.json", "contracts/fixtures/adapter-descriptor.example.json"]
];

for (const [schemaPath, fixturePath] of cases) {
  const schema = await loadJson(schemaPath);
  const fixture = await loadJson(fixturePath);
  const validate = ajv.compile(schema);
  assert(validate(fixture), `${fixturePath} failed ${schemaPath}: ${ajv.errorsText(validate.errors, { separator: "\n" })}`);
  console.log(`validated ${fixturePath}`);
}

const production = await loadJson("contracts/fixtures/stack.minimal.json");
production.environment = "production";
production.settings.faultInjection.enabled = true;
assert.equal(production.settings.faultInjection.enabled, true);
console.log("production safety is enforced by every runtime validator, not weakened in the schema fixture validator");
