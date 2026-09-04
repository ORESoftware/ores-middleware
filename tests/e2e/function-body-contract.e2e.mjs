import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import {
  executeOperationPlan,
  REQUIRED_SEQUENCE,
  validatePlanSemantics,
  validateSourceBindings,
} from "../../scripts/lib/function-body-contracts.mjs";
import { applyJsonPointerMutations } from "../../scripts/lib/json-pointer-mutations.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const plan = await readJson("contracts/function-bodies/operation-boundary.plan.json");
const bindings = await readJson("contracts/function-bodies/language-bindings.json");
const negativeCases = await readJson("tests/e2e/fixtures/function-body-negative-cases.json");

async function readJson(relative) {
  return JSON.parse(await readFile(path.join(root, relative), "utf8"));
}

test("canonical pseudocode body is complete, ordered, and acyclic", () => {
  assert.deepEqual(validatePlanSemantics(plan), []);
  assert.deepEqual(plan.steps.map((step) => step.opcode), REQUIRED_SEQUENCE);
});

test("successful operation restores context and never enters failure-only steps", () => {
  const state = executeOperationPlan(plan, {kind: "success", value: {accepted: true}});
  assert.deepEqual(state.outcome, {ok: true, value: {accepted: true}});
  assert.equal(state.operationInvoked, true);
  assert.equal(state.contextRestored, true);
  assert.equal(state.reportAttempts, 0);
  assert.equal(state.trace.includes("redact-public-failure"), false);
});

for (const [kind, code, invoked] of [
  ["error", "operation_failed", true],
  ["panic", "operation_panicked", true],
  ["cancelled", "operation_cancelled", false],
  ["deadline-exceeded", "operation_deadline_exceeded", false],
]) {
  test(`${kind} becomes a bounded typed outcome`, () => {
    const privateValues = {
      rawCause: `private-cause-${kind}`,
      rawMessage: `private-message-${kind}`,
      rawStack: `private-stack-${kind}`,
      rawBody: `private-body-${kind}`,
    };
    const state = executeOperationPlan(plan, {kind, ...privateValues});
    assert.equal(state.outcome.ok, false);
    assert.equal(state.outcome.failure.code, code);
    assert.equal(state.operationInvoked, invoked);
    assert.equal(state.contextRestored, true);
    assert.equal(state.reportAttempts, 1);
    const serialized = JSON.stringify(state.outcome);
    for (const value of Object.values(privateValues)) assert.equal(serialized.includes(value), false);
    for (const field of plan.forbiddenPublicFields) assert.equal(Object.hasOwn(state.outcome.failure, field), false);
  });
}

test("failure reporter exceptions are fail-open", () => {
  const state = executeOperationPlan(plan, {kind: "error", reporterThrows: true});
  assert.equal(state.outcome.ok, false);
  assert.equal(state.outcome.failure.code, "operation_failed");
  assert.equal(state.reporterFailureIgnored, true);
  assert.equal(state.contextRestored, true);
});

test("unsafe operation and error names normalize to bounded low-cardinality values", () => {
  const state = executeOperationPlan(plan, {
    kind: "error",
    operationName: "customer/123?authorization=secret",
    errorType: "Error<private-message>",
  });
  assert.equal(state.outcome.failure.operation, "operation");
  assert.equal(state.outcome.failure.errorType, "error");
});

for (const testCase of negativeCases.semanticCases) {
  test(`semantic mutation stops evaluation: ${testCase.name}`, () => {
    const candidate = applyJsonPointerMutations(plan, testCase.mutations);
    const codes = new Set(validatePlanSemantics(candidate).map((finding) => finding.code));
    for (const expected of testCase.expectedCodes) assert.equal(codes.has(expected), true, `${testCase.name}: missing ${expected}; got ${JSON.stringify([...codes])}`);
  });
}

test("JSON Pointer mutations target the requested array rather than its parent object", () => {
  const candidate = applyJsonPointerMutations(plan, [
    {op: "swap", path: "/steps", value: [3, 5]},
  ]);
  assert.equal(candidate.steps[3].opcode, "invoke-operation");
  assert.equal(candidate.steps[5].opcode, "enter-failure-boundary");
  assert.equal(plan.steps[3].opcode, "enter-failure-boundary");
});

test("mutation engine rejects traversal, invalid indexes, and non-array swaps", () => {
  assert.throws(
    () => applyJsonPointerMutations(plan, [{op: "set", path: "/steps/99/opcode", value: "invoke-operation"}]),
    /out of range|does not exist/,
  );
  assert.throws(
    () => applyJsonPointerMutations(plan, [{op: "delete", path: "/missing", value: null}]),
    /does not exist/,
  );
  assert.throws(
    () => applyJsonPointerMutations(plan, [{op: "swap", path: "/function", value: [0, 1]}]),
    /not an array/,
  );
});

test("every language binds every pseudocode step to reviewed source witnesses", async () => {
  const result = await validateSourceBindings(root, plan, bindings);
  assert.deepEqual(result.findings, []);
  assert.equal(Object.keys(result.sourceDigests).length >= 8, true);
  for (const digest of Object.values(result.sourceDigests)) assert.match(digest, /^[0-9a-f]{64}$/);
});
