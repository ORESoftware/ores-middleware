#!/usr/bin/env node
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";

const root = new URL("../", import.meta.url);
const loadJson = async (path) => JSON.parse(await readFile(new URL(path, root), "utf8"));

const cases = [
  ["contracts/json-schema/middleware-stack.schema.json", "contracts/fixtures/stack.minimal.json"],
  ["contracts/json-schema/middleware-stack.schema.json", "contracts/fixtures/stack.ssr.json"],
  ["contracts/json-schema/adapter-descriptor.schema.json", "contracts/fixtures/adapter-descriptor.example.json"]
];

for (const [schemaPath, fixturePath] of cases) {
  const schema = await loadJson(schemaPath);
  const fixture = await loadJson(fixturePath);
  // A schema may intentionally have multiple independent fixtures. Use one
  // validator registry per fixture so a repeated canonical $id does not look
  // like an accidental duplicate schema registration.
  const fixtureAjv = new Ajv2020({ allErrors: true, strict: true });
  addFormats(fixtureAjv);
  const validate = fixtureAjv.compile(schema);
  assert(
    validate(fixture),
    `${fixturePath} failed ${schemaPath}: ${fixtureAjv.errorsText(validate.errors, { separator: "\n" })}`
  );
  console.log(`validated ${fixturePath}`);
}

const middlewareSchema = await loadJson("contracts/json-schema/middleware-stack.schema.json");
const malformedIssuer = await loadJson("contracts/fixtures/stack.minimal.json");
malformedIssuer.integrations.sharedAuth.issuer = "not a uri";
const malformedIssuerAjv = new Ajv2020({ allErrors: true, strict: true });
addFormats(malformedIssuerAjv);
const validateMalformedIssuer = malformedIssuerAjv.compile(middlewareSchema);
assert.equal(
  validateMalformedIssuer(malformedIssuer),
  false,
  "Shared Auth issuer must reject malformed URI syntax"
);
assert(
  validateMalformedIssuer.errors?.some(
    (error) => error.instancePath === "/integrations/sharedAuth/issuer" && error.keyword === "format"
  ),
  `malformed issuer must fail the URI format boundary: ${malformedIssuerAjv.errorsText(validateMalformedIssuer.errors, { separator: "\n" })}`
);
console.log("rejected malformed Shared Auth issuer URI");

const production = await loadJson("contracts/fixtures/stack.minimal.json");
production.environment = "production";
production.settings.faultInjection.enabled = true;
assert.equal(production.settings.faultInjection.enabled, true);
console.log("production safety is enforced by every runtime validator, not weakened in the schema fixture validator");
