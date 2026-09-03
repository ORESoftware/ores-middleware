import fs from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";

const root = process.cwd();
const typespecPath = path.join(root, "contracts/rate-limit-v2/typespec/main.tsp");
const schemaPath = path.join(
  root,
  "contracts/rate-limit-v2/json-schema/rate-limit-policy.schema.json",
);
const validDirectory = path.join(root, "contracts/rate-limit-v2/fixtures/valid");
const invalidDirectory = path.join(root, "contracts/rate-limit-v2/fixtures/invalid");
const receiptPath = path.join(root, "target/rate-limit-v2/receipt.json");

await main();

async function main() {
  try {
    const receipt = await validateContracts();
    await writeReceipt(receipt);
    console.log(JSON.stringify(receipt));
  } catch (error) {
    const detail = describeError(error);
    const receipt = {
      schema: "ores.middleware.rate-limit-v2-contract-gate/v1",
      status: "stopped_for_evaluation",
      zeroUnexplainedFindings: false,
      authorities: {
        typeSpec: path.relative(root, typespecPath),
        jsonSchema: path.relative(root, schemaPath),
        relationship: "independent-peers",
      },
      findings: [
        {
          code: "contract-gate-failed",
          state: "unexplained",
          detail,
        },
      ],
    };
    await writeReceipt(receipt);
    console.error(detail);
    process.exitCode = 1;
  }
}

async function validateContracts() {
  const [typespec, schemaText] = await Promise.all([
    fs.readFile(typespecPath, "utf8"),
    fs.readFile(schemaPath, "utf8"),
  ]);
  const schema = JSON.parse(schemaText);

  assert(
    typespec.includes("Independently authored rate-limit policy authority"),
    "TypeSpec authority marker is missing",
  );
  assert(
    schema.description?.includes("Independently authored JSON Schema authority"),
    "JSON Schema authority marker is missing",
  );
  assert(
    schema.$schema === "https://json-schema.org/draft/2020-12/schema",
    "JSON Schema must declare Draft 2020-12",
  );

  const enumPairs = [
    ["RateLimitAlgorithmV2", "rateLimitAlgorithmV2"],
    ["RateLimitConsistency", "rateLimitConsistency"],
    ["RateLimitEnforcementMode", "rateLimitEnforcementMode"],
    ["RateLimitFailureMode", "rateLimitFailureMode"],
    ["RateLimitLayer", "rateLimitLayer"],
    ["OperationClass", "operationClass"],
  ];

  const enumReceipt = {};
  for (const [typespecName, schemaName] of enumPairs) {
    const left = parseTypeSpecEnum(typespec, typespecName);
    const definition = schema.$defs?.[schemaName];
    assert(
      definition && Array.isArray(definition.enum),
      `JSON Schema enum ${schemaName} is missing`,
    );
    const right = [...definition.enum].sort();
    assertEqualSets(left, right, `${typespecName}/${schemaName}`);
    enumReceipt[typespecName] = left;
  }

  const typespecProperties = parseTypeSpecModelProperties(typespec, "RateLimitPolicyV2");
  const schemaPropertyObject = schema.properties;
  assert(
    schemaPropertyObject && typeof schemaPropertyObject === "object",
    "JSON Schema RateLimitPolicyV2 properties are missing",
  );
  const schemaProperties = Object.keys(schemaPropertyObject).sort();
  assertEqualSets(typespecProperties, schemaProperties, "RateLimitPolicyV2 properties");

  for (const forbidden of [
    "email",
    "ip",
    "subject",
    "tenant",
    "user",
    "rawIdentity",
    "bearerToken",
    "cookie",
  ]) {
    assert(
      !schemaProperties.includes(forbidden),
      `raw identity field ${forbidden} is forbidden in rate-limit policy contracts`,
    );
  }

  const ajv = new Ajv2020({
    allErrors: true,
    strict: true,
    validateFormats: true,
  });
  addFormats(ajv);
  const validate = ajv.compile(schema);

  const validResults = await validateDirectory(
    validDirectory,
    true,
    validate,
    ajv,
  );
  const invalidResults = await validateDirectory(
    invalidDirectory,
    false,
    validate,
    ajv,
  );

  return {
    schema: "ores.middleware.rate-limit-v2-contract-gate/v1",
    status: "passed",
    zeroUnexplainedFindings: true,
    authorities: {
      typeSpec: path.relative(root, typespecPath),
      jsonSchema: path.relative(root, schemaPath),
      relationship: "independent-peers",
    },
    enums: enumReceipt,
    properties: schemaProperties,
    validFixtures: validResults,
    invalidFixtures: invalidResults,
    findings: [],
  };
}

async function writeReceipt(receipt) {
  await fs.mkdir(path.dirname(receiptPath), { recursive: true });
  await fs.writeFile(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`);
}

async function validateDirectory(directory, expectedValidity, validate, ajv) {
  const names = (await fs.readdir(directory))
    .filter((name) => name.endsWith(".json"))
    .sort();
  assert(names.length > 0, `no JSON fixtures found in ${directory}`);

  const results = [];
  for (const name of names) {
    const value = JSON.parse(await fs.readFile(path.join(directory, name), "utf8"));
    const valid = validate(value);
    if (valid !== expectedValidity) {
      const detail = ajv.errorsText(validate.errors, { separator: "; " });
      throw new Error(
        `${name} validity was ${String(valid)}, expected ${String(expectedValidity)}: ${detail}`,
      );
    }
    results.push({
      file: name,
      expectedValidity,
      errorKeywords: expectedValidity
        ? []
        : [...new Set((validate.errors ?? []).map((error) => error.keyword))].sort(),
    });
  }
  return results;
}

function parseTypeSpecEnum(source, name) {
  const match = source.match(new RegExp(`enum\\s+${name}\\s*\\{([\\s\\S]*?)\\}`));
  assert(match, `TypeSpec enum ${name} is missing`);
  return [...match[1].matchAll(/:\s*"([^"]+)"/g)]
    .map((entry) => entry[1])
    .sort();
}

function parseTypeSpecModelProperties(source, name) {
  const match = source.match(new RegExp(`model\\s+${name}\\s*\\{([\\s\\S]*?)\\}`));
  assert(match, `TypeSpec model ${name} is missing`);
  return [...match[1].matchAll(/^\s*([A-Za-z][A-Za-z0-9]*)\??\s*:/gm)]
    .map((entry) => entry[1])
    .sort();
}

function assertEqualSets(left, right, label) {
  assert(
    JSON.stringify(left) === JSON.stringify(right),
    `${label} parity mismatch: TypeSpec=${JSON.stringify(left)} JSON Schema=${JSON.stringify(right)}`,
  );
}

function describeError(error) {
  const message = error instanceof Error ? error.message : String(error);
  return message.replaceAll(root, ".").slice(0, 2000);
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}
