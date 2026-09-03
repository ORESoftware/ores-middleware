import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";
import {
  applyMutations,
  comparePeerAuthorities,
  executeOperationPlan,
  validatePlanSemantics,
  validateSourceBindings,
} from "./lib/function-body-contracts.mjs";

const root = process.cwd();
const paths = {
  typeSpec: "contracts/function-bodies/typespec/main.tsp",
  planSchema: "contracts/function-bodies/json-schema/function-body-plan.schema.json",
  bindingSchema: "contracts/function-bodies/json-schema/language-body-bindings.schema.json",
  plan: "contracts/function-bodies/operation-boundary.plan.json",
  bindings: "contracts/function-bodies/language-bindings.json",
  negativeCases: "tests/e2e/fixtures/function-body-negative-cases.json",
  receipt: "target/function-body-contract/receipt.json",
};

const startedAt = new Date().toISOString();
await main();

async function main() {
  let status = "failed";
  let findings = [];
  let evidence = {};
  try {
    const [typeSpec, planSchema, bindingSchema, plan, bindings, negativeCases] = await Promise.all([
      readText(paths.typeSpec),
      readJson(paths.planSchema),
      readJson(paths.bindingSchema),
      readJson(paths.plan),
      readJson(paths.bindings),
      readJson(paths.negativeCases),
    ]);

    const ajv = new Ajv2020({allErrors: true, strict: true, validateFormats: true});
    addFormats(ajv);
    const validatePlan = ajv.compile(planSchema);
    const validateBindings = ajv.compile(bindingSchema);

    findings.push(...schemaFindings(validatePlan, plan, paths.plan));
    findings.push(...schemaFindings(validateBindings, bindings, paths.bindings));
    findings.push(...comparePeerAuthorities(typeSpec, planSchema, bindingSchema));
    findings.push(...validatePlanSemantics(plan));

    const sourceResult = await validateSourceBindings(root, plan, bindings);
    findings.push(...sourceResult.findings);

    const schemaCases = [];
    for (const testCase of negativeCases.schemaCases ?? []) {
      const candidate = applyMutations(plan, testCase.mutations ?? []);
      const accepted = validatePlan(candidate);
      const keywords = [...new Set((validatePlan.errors ?? []).map((error) => error.keyword))].sort();
      if (accepted) {
        findings.push({code: "negative-schema-case-accepted", location: paths.negativeCases, detail: testCase.name});
      }
      for (const keyword of testCase.expectedKeywords ?? []) {
        if (!keywords.includes(keyword)) findings.push({code: "negative-schema-keyword-missing", location: paths.negativeCases, detail: `${testCase.name}: expected ${keyword}, got ${JSON.stringify(keywords)}`});
      }
      schemaCases.push({name: testCase.name, accepted, keywords});
    }

    const semanticCases = [];
    for (const testCase of negativeCases.semanticCases ?? []) {
      const candidate = applyMutations(plan, testCase.mutations ?? []);
      const candidateFindings = validatePlanSemantics(candidate);
      const codes = [...new Set(candidateFindings.map((item) => item.code))].sort();
      for (const code of testCase.expectedCodes ?? []) {
        if (!codes.includes(code)) findings.push({code: "negative-semantic-code-missing", location: paths.negativeCases, detail: `${testCase.name}: expected ${code}, got ${JSON.stringify(codes)}`});
      }
      semanticCases.push({name: testCase.name, codes});
    }

    const scenarios = [
      {name: "success", input: {kind: "success", value: "accepted"}, expectedCode: null, invoked: true},
      {name: "error", input: {kind: "error", rawMessage: "private-message", rawStack: "private-stack"}, expectedCode: "operation_failed", invoked: true},
      {name: "panic", input: {kind: "panic", rawCause: "private-cause"}, expectedCode: "operation_panicked", invoked: true},
      {name: "cancelled", input: {kind: "cancelled"}, expectedCode: "operation_cancelled", invoked: false},
      {name: "deadline", input: {kind: "deadline-exceeded"}, expectedCode: "operation_deadline_exceeded", invoked: false},
      {name: "reporter-failure", input: {kind: "error", reporterThrows: true}, expectedCode: "operation_failed", invoked: true},
      {name: "unsafe-name", input: {kind: "error", operationName: "customer/secret"}, expectedCode: "operation_failed", invoked: true},
    ];
    const scenarioResults = [];
    for (const scenario of scenarios) {
      const result = executeOperationPlan(plan, scenario.input);
      const code = result.outcome.ok ? null : result.outcome.failure.code;
      if (code !== scenario.expectedCode || result.operationInvoked !== scenario.invoked || !result.contextRestored) {
        findings.push({code: "pseudocode-scenario-mismatch", location: `/scenarios/${scenario.name}`, detail: `code=${String(code)} invoked=${String(result.operationInvoked)} restored=${String(result.contextRestored)}`});
      }
      if (scenario.name === "reporter-failure" && !result.reporterFailureIgnored) findings.push({code: "reporter-failure-not-ignored", location: `/scenarios/${scenario.name}`, detail: "reporter failure changed control flow"});
      if (scenario.name === "unsafe-name" && result.outcome.failure.operation !== "operation") findings.push({code: "unsafe-operation-name", location: `/scenarios/${scenario.name}`, detail: "unsafe operation name was not normalized"});
      scenarioResults.push({name: scenario.name, code, invoked: result.operationInvoked, restored: result.contextRestored, trace: result.trace});
    }

    findings = stableFindings(findings);
    status = findings.length === 0 ? "passed" : "stopped_for_evaluation";
    evidence = {
      authorityDigests: {
        typeSpec: sha256(typeSpec),
        jsonSchema: sha256(JSON.stringify(planSchema)),
        bindingSchema: sha256(JSON.stringify(bindingSchema)),
      },
      planDigest: sha256(JSON.stringify(plan)),
      sourceDigests: sourceResult.sourceDigests,
      negativeSchemaCases: schemaCases,
      negativeSemanticCases: semanticCases,
      scenarios: scenarioResults,
    };
  } catch (error) {
    findings = [{code: "audit-infrastructure-failure", location: "/", detail: boundedError(error)}];
    status = "failed";
  }

  const receipt = {
    schema: "ores.middleware.function-body-audit-receipt/v1",
    startedAt,
    completedAt: new Date().toISOString(),
    commit: process.env.GITHUB_SHA ?? null,
    status,
    zeroUnexplainedFindings: status === "passed",
    authorities: {
      typeSpec: paths.typeSpec,
      jsonSchema: paths.planSchema,
      relationship: "independent-top-level-peers",
    },
    plan: paths.plan,
    bindings: paths.bindings,
    evidence,
    findings,
  };
  await mkdir(path.dirname(path.join(root, paths.receipt)), {recursive: true});
  await writeFile(path.join(root, paths.receipt), `${JSON.stringify(receipt, null, 2)}\n`, "utf8");
  console.log(JSON.stringify(receipt));
  if (status !== "passed") process.exitCode = status === "stopped_for_evaluation" ? 2 : 1;
}

async function readText(relative) {
  return readFile(path.join(root, relative), "utf8");
}
async function readJson(relative) {
  return JSON.parse(await readText(relative));
}
function schemaFindings(validate, value, location) {
  if (validate(value)) return [];
  return (validate.errors ?? []).map((error) => ({code: `json-schema-${error.keyword}`, location: `${location}${error.instancePath}`, detail: error.message ?? "schema validation failed"}));
}
function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}
function stableFindings(items) {
  return items
    .map((item) => ({code: String(item.code), location: String(item.location), detail: String(item.detail).slice(0, 2000)}))
    .sort((left, right) => `${left.code}:${left.location}:${left.detail}`.localeCompare(`${right.code}:${right.location}:${right.detail}`));
}
function boundedError(error) {
  const message = error instanceof Error ? error.message : String(error);
  return message.replaceAll(root, ".").slice(0, 2000);
}
