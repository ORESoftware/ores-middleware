#!/usr/bin/env node
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const root = new URL("../", import.meta.url);
const tsp = await readFile(new URL("contracts/typespec/main.tsp", root), "utf8");
const stack = JSON.parse(await readFile(new URL("contracts/json-schema/middleware-stack.schema.json", root), "utf8"));
const descriptor = JSON.parse(await readFile(new URL("contracts/json-schema/adapter-descriptor.schema.json", root), "utf8"));

function typeSpecEnum(name) {
  const match = tsp.match(new RegExp(`enum\\s+${name}\\s*\\{([\\s\\S]*?)\\n\\}`, "m"));
  assert(match, `missing TypeSpec enum ${name}`);
  return [...match[1].matchAll(/:\s*"([^"]+)"/g)].map((item) => item[1]);
}

function sameSet(label, left, right) {
  const a = [...new Set(left)].sort();
  const b = [...new Set(right)].sort();
  assert.deepEqual(a, b, `${label} discrepancy\nTypeSpec: ${a.join(", ")}\nJSON Schema: ${b.join(", ")}`);
}

const tspCapabilities = typeSpecEnum("MiddlewareCapability");
const stackCapabilities = stack.$defs.capability.enum;
const descriptorCapabilityRef = descriptor.$defs.adapterDescriptor.properties.capabilities.items.$ref;
assert.equal(
  descriptorCapabilityRef,
  "https://schemas.oresoftware.com/middleware/v1/middleware-stack.schema.json#/$defs/capability",
  "adapter descriptor capabilities must reference the canonical middleware capability authority"
);
sameSet("capability authority", tspCapabilities, stackCapabilities);
sameSet("descriptor capability projection", tspCapabilities, stackCapabilities);

const tspOperations = typeSpecEnum("SdkOperation");
const schemaOperations = descriptor.$defs.sdkOperation.enum;
sameSet("SDK operation authority", tspOperations, schemaOperations);

const tspRepresentations = typeSpecEnum("ContentRepresentation");
const schemaRepresentations = stack.$defs.contentRepresentation.enum;
sameSet("content representation authority", tspRepresentations, schemaRepresentations);
assert(tspRepresentations.includes("text/html"), "SSR web servers require text/html in the peer authorities");

console.log(
  `authority parity ok: ${tspCapabilities.length} capabilities, ${tspOperations.length} SDK operations, ${tspRepresentations.length} content representations`
);
