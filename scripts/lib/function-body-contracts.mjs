import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";

export const REQUIRED_LANGUAGES = Object.freeze(["rust", "typescript", "golang", "gleam", "elixir", "erlang"]);
export const REQUIRED_SEQUENCE = Object.freeze([
  "capture-context",
  "normalize-input",
  "enter-context",
  "enter-failure-boundary",
  "arm-termination-guard",
  "invoke-operation",
  "classify-failure",
  "redact-public-failure",
  "report-failure-fail-open",
  "restore-context",
  "return-typed-outcome",
]);

const REQUIRED_PUBLIC_FIELDS = Object.freeze(["kind", "code", "transport", "scope", "operation", "requestId", "traceId", "errorType"]);
const REQUIRED_FORBIDDEN_FIELDS = Object.freeze(["cause", "message", "stack", "body", "authorization", "cookie", "token", "rawIdentity"]);
const STEP_RULES = Object.freeze({
  "capture-context": {phase: "setup", when: "always", failurePolicy: "map-to-typed-failure", effects: ["read-ambient-context", "snapshot-allowlisted-context"]},
  "normalize-input": {phase: "setup", when: "always", failurePolicy: "map-to-typed-failure", effects: ["normalize-low-cardinality-input"]},
  "enter-context": {phase: "setup", when: "always", failurePolicy: "map-to-typed-failure", effects: ["install-request-context"]},
  "enter-failure-boundary": {phase: "guarded", when: "always", failurePolicy: "map-to-typed-failure", effects: ["catch-language-failure"]},
  "arm-termination-guard": {phase: "guarded", when: "always", failurePolicy: "map-to-typed-failure", effects: ["observe-cancellation-or-deadline"]},
  "invoke-operation": {phase: "guarded", when: "unless-failed", failurePolicy: "map-to-typed-failure", effects: ["invoke-user-code"]},
  "classify-failure": {phase: "failure", when: "on-failure", failurePolicy: "map-to-typed-failure", effects: ["classify-failure-without-payload"]},
  "redact-public-failure": {phase: "failure", when: "on-failure", failurePolicy: "map-to-typed-failure", effects: ["construct-public-failure"]},
  "report-failure-fail-open": {phase: "failure", when: "on-failure", failurePolicy: "ignore-reporter-failure", effects: ["emit-bounded-telemetry"]},
  "restore-context": {phase: "finalization", when: "finally", failurePolicy: "always-run", effects: ["restore-ambient-context"]},
  "return-typed-outcome": {phase: "terminal", when: "always", failurePolicy: "not-applicable", effects: ["return-outcome"]},
});

function finding(code, location, detail) {
  return { code, location, detail };
}
function sortedUnique(values) {
  return [...new Set(values)].sort();
}
function sameSet(left, right) {
  return JSON.stringify(sortedUnique(left)) === JSON.stringify(sortedUnique(right));
}

export function validatePlanSemantics(plan) {
  const findings = [];
  if (!plan || typeof plan !== "object" || Array.isArray(plan)) return [finding("plan-type", "/", "plan must be an object")];
  const steps = Array.isArray(plan.steps) ? plan.steps : [];
  const opcodes = steps.map((step) => step?.opcode);
  if (JSON.stringify(opcodes) !== JSON.stringify(REQUIRED_SEQUENCE)) {
    findings.push(finding("sequence-mismatch", "/steps", `expected ${JSON.stringify(REQUIRED_SEQUENCE)}, got ${JSON.stringify(opcodes)}`));
  }
  const ids = steps.map((step) => step?.id);
  if (new Set(ids).size !== ids.length) findings.push(finding("duplicate-step-id", "/steps", "step identifiers must be unique"));
  if (new Set(opcodes).size !== opcodes.length) findings.push(finding("duplicate-opcode", "/steps", "opcodes must occur exactly once"));

  const idToIndex = new Map(ids.map((id, index) => [id, index]));
  for (const [index, step] of steps.entries()) {
    if (!step || typeof step !== "object") {
      findings.push(finding("step-type", `/steps/${index}`, "step must be an object"));
      continue;
    }
    if (step.id !== step.opcode) findings.push(finding("step-id-opcode-mismatch", `/steps/${index}`, "step id must equal its opcode"));
    const rule = STEP_RULES[step.opcode];
    if (!rule) continue;
    for (const key of ["phase", "when", "failurePolicy"]) {
      if (step[key] !== rule[key]) findings.push(finding(`step-${key}-mismatch`, `/steps/${index}/${key}`, `${step.opcode} requires ${rule[key]}`));
    }
    if (!sameSet(step.effects ?? [], rule.effects)) findings.push(finding("step-effects-mismatch", `/steps/${index}/effects`, `${step.opcode} requires exactly ${JSON.stringify(rule.effects)}`));
    for (const dependency of step.dependsOn ?? []) {
      const dependencyIndex = idToIndex.get(dependency);
      if (dependencyIndex === undefined) findings.push(finding("unknown-dependency", `/steps/${index}/dependsOn`, `${step.id} depends on unknown step ${dependency}`));
      else if (dependencyIndex >= index) findings.push(finding("dependency-order", `/steps/${index}/dependsOn`, `${step.id} depends on non-prior step ${dependency}`));
    }
  }
  const terminal = steps.filter((step) => step?.terminal === true);
  if (terminal.length !== 1 || terminal[0]?.opcode !== "return-typed-outcome") findings.push(finding("terminal-step", "/steps", "return-typed-outcome must be the only terminal step"));
  if (!sameSet(plan.publicFailureFields ?? [], REQUIRED_PUBLIC_FIELDS)) findings.push(finding("public-failure-fields", "/publicFailureFields", `public failure fields must be exactly ${JSON.stringify(REQUIRED_PUBLIC_FIELDS)}`));
  if (!REQUIRED_FORBIDDEN_FIELDS.every((field) => plan.forbiddenPublicFields?.includes(field))) findings.push(finding("forbidden-public-fields", "/forbiddenPublicFields", "raw causes, messages, stacks, bodies, credentials, and identities must be forbidden"));
  for (const field of plan.publicFailureFields ?? []) {
    if (REQUIRED_FORBIDDEN_FIELDS.includes(field)) findings.push(finding("public-field-forbidden", "/publicFailureFields", `${field} cannot be exposed by the public failure value`));
  }
  return findings.sort((left, right) => `${left.code}:${left.location}:${left.detail}`.localeCompare(`${right.code}:${right.location}:${right.detail}`));
}

function shouldRun(when, state) {
  if (when === "always" || when === "finally") return true;
  if (when === "unless-failed") return state.failureKind === undefined;
  if (when === "on-failure") return state.failureKind !== undefined;
  throw new Error(`unknown pseudocode condition: ${String(when)}`);
}
function failureCode(kind) {
  return {error: "operation_failed", panic: "operation_panicked", cancelled: "operation_cancelled", "deadline-exceeded": "operation_deadline_exceeded"}[kind];
}
function boundedToken(value, fallback, maximum) {
  return typeof value === "string" && value.length > 0 && value.length <= maximum && /^[A-Za-z0-9_.:-]+$/.test(value) ? value : fallback;
}

export function executeOperationPlan(plan, scenario = {}) {
  const findings = validatePlanSemantics(plan);
  if (findings.length > 0) throw new Error(`cannot execute invalid function body plan: ${findings[0].code}`);
  const kind = scenario.kind ?? "success";
  const state = {contextCaptured: false, contextEntered: false, contextRestored: false, boundaryEntered: false, terminationGuardArmed: false, operationInvoked: false, operationName: undefined, failureKind: undefined, failureCode: undefined, publicFailure: undefined, reportAttempts: 0, reporterFailureIgnored: false, trace: [], outcome: undefined};
  for (const step of plan.steps) {
    if (!shouldRun(step.when, state)) continue;
    state.trace.push(step.opcode);
    switch (step.opcode) {
      case "capture-context": state.contextCaptured = true; break;
      case "normalize-input": state.operationName = boundedToken(scenario.operationName ?? "orders.read", "operation", 128); break;
      case "enter-context": state.contextEntered = true; break;
      case "enter-failure-boundary": state.boundaryEntered = true; break;
      case "arm-termination-guard":
        state.terminationGuardArmed = true;
        if (kind === "cancelled" || kind === "deadline-exceeded") state.failureKind = kind;
        break;
      case "invoke-operation":
        state.operationInvoked = true;
        if (kind === "error" || kind === "panic") state.failureKind = kind;
        break;
      case "classify-failure": state.failureCode = failureCode(state.failureKind); break;
      case "redact-public-failure":
        state.publicFailure = {kind: state.failureKind, code: state.failureCode, transport: scenario.transport ?? "http", scope: scenario.scope ?? "request", operation: state.operationName, requestId: scenario.requestId ?? "request-1", traceId: scenario.traceId ?? "00000000000000000000000000000001", errorType: boundedToken(scenario.errorType ?? "OperationError", "error", 64)};
        break;
      case "report-failure-fail-open":
        state.reportAttempts += 1;
        try { if (scenario.reporterThrows === true) throw new Error("reporter failed"); } catch { state.reporterFailureIgnored = true; }
        break;
      case "restore-context": state.contextEntered = false; state.contextRestored = true; break;
      case "return-typed-outcome":
        if (!state.contextRestored) throw new Error("context was not restored before return");
        state.outcome = state.publicFailure ? {ok: false, failure: state.publicFailure} : {ok: true, value: scenario.value ?? "ok"};
        break;
      default: throw new Error(`unsupported opcode ${step.opcode}`);
    }
  }
  if (!state.outcome) throw new Error("pseudocode body produced no terminal outcome");
  const serialized = JSON.stringify(state.outcome);
  for (const forbidden of plan.forbiddenPublicFields) if (Object.hasOwn(state.publicFailure ?? {}, forbidden)) throw new Error(`public failure exposed forbidden field ${forbidden}`);
  for (const secret of [scenario.rawCause, scenario.rawMessage, scenario.rawStack, scenario.rawBody]) {
    if (typeof secret === "string" && secret.length > 0 && serialized.includes(secret)) throw new Error("public outcome copied a raw private failure value");
  }
  return state;
}

function parseTypeSpecEnum(source, name) {
  const match = source.match(new RegExp(`enum\\s+${name}\\s*\\{([\\s\\S]*?)\\}`));
  return match ? [...match[1].matchAll(/:\s*"([^"]+)"/g)].map((entry) => entry[1]).sort() : undefined;
}
function parseTypeSpecModelProperties(source, name) {
  const match = source.match(new RegExp(`model\\s+${name}\\s*\\{([\\s\\S]*?)\\}`));
  return match ? [...match[1].matchAll(/^\s*([A-Za-z][A-Za-z0-9]*)\??\s*:/gm)].map((entry) => entry[1]).sort() : undefined;
}
function parseTypeSpecStringUnion(source, modelName, propertyName) {
  const model = source.match(new RegExp(`model\\s+${modelName}\\s*\\{([\\s\\S]*?)\\}`));
  const property = model?.[1].match(new RegExp(`${propertyName}\\s*:\\s*([^;]+);`));
  return property ? [...property[1].matchAll(/"([^"]+)"/g)].map((entry) => entry[1]).sort() : undefined;
}

export function comparePeerAuthorities(typespec, planSchema, bindingSchema) {
  const findings = [];
  if (!typespec.includes("Independently authored compile-time authority")) findings.push(finding("typespec-authority-marker", "/typespec", "TypeSpec peer-authority marker missing"));
  if (!planSchema.description?.includes("Independently authored runtime authority")) findings.push(finding("json-schema-authority-marker", "/json-schema", "JSON Schema peer-authority marker missing"));
  for (const [name, schemaValues] of [["BodyOpcode", planSchema.$defs?.opcode?.enum], ["BodyPhase", planSchema.$defs?.phase?.enum], ["BodyWhen", planSchema.$defs?.when?.enum], ["BodyEffect", planSchema.$defs?.effect?.enum], ["BodyFailurePolicy", planSchema.$defs?.failurePolicy?.enum]]) {
    const typeSpecValues = parseTypeSpecEnum(typespec, name);
    if (!typeSpecValues || !Array.isArray(schemaValues) || !sameSet(typeSpecValues, schemaValues)) findings.push(finding("peer-enum-mismatch", `/authorities/${name}`, `TypeSpec=${JSON.stringify(typeSpecValues)} JSON-Schema=${JSON.stringify(schemaValues)}`));
  }
  for (const [name, properties] of [["FunctionBodyStep", planSchema.$defs?.step?.properties], ["FunctionBodyPlan", planSchema.properties], ["BodyStepEvidence", bindingSchema.$defs?.stepEvidence?.properties], ["LanguageBodyBinding", bindingSchema.$defs?.binding?.properties], ["FunctionBodyBindings", bindingSchema.properties]]) {
    const typeSpecProperties = parseTypeSpecModelProperties(typespec, name);
    const schemaProperties = properties ? Object.keys(properties).sort() : undefined;
    if (!typeSpecProperties || !schemaProperties || !sameSet(typeSpecProperties, schemaProperties)) findings.push(finding("peer-model-mismatch", `/authorities/${name}`, `TypeSpec=${JSON.stringify(typeSpecProperties)} JSON-Schema=${JSON.stringify(schemaProperties)}`));
  }
  const typeSpecLanguages = parseTypeSpecStringUnion(typespec, "LanguageBodyBinding", "language");
  const schemaLanguages = bindingSchema.$defs?.binding?.properties?.language?.enum;
  if (!typeSpecLanguages || !Array.isArray(schemaLanguages) || !sameSet(typeSpecLanguages, schemaLanguages)) findings.push(finding("peer-language-mismatch", "/authorities/LanguageBodyBinding/language", `TypeSpec=${JSON.stringify(typeSpecLanguages)} JSON-Schema=${JSON.stringify(schemaLanguages)}`));
  return findings;
}

function safeResolve(root, relative) {
  const rootPath = path.resolve(root);
  const resolved = path.resolve(rootPath, relative);
  if (resolved !== rootPath && !resolved.startsWith(`${rootPath}${path.sep}`)) throw new Error(`source path escapes repository root: ${relative}`);
  return resolved;
}
function digest(content) {
  return createHash("sha256").update(content).digest("hex");
}

export async function validateSourceBindings(root, plan, bindingDocument) {
  const findings = [];
  const sourceDigests = {};
  if (bindingDocument.contractId !== plan.contractId) findings.push(finding("binding-contract-id", "/bindings/contractId", "binding contract id must match plan"));
  const bindings = Array.isArray(bindingDocument.bindings) ? bindingDocument.bindings : [];
  const languages = bindings.map((binding) => binding.language);
  if (!sameSet(languages, REQUIRED_LANGUAGES) || new Set(languages).size !== languages.length) findings.push(finding("language-set", "/bindings", `bindings must contain exactly ${JSON.stringify(REQUIRED_LANGUAGES)}`));
  for (const [bindingIndex, binding] of bindings.entries()) {
    const sources = new Map();
    for (const relative of binding.sources ?? []) {
      try {
        const content = await readFile(safeResolve(root, relative), "utf8");
        sources.set(relative, content);
        sourceDigests[relative] = digest(content);
      } catch (error) {
        findings.push(finding("source-read", `/bindings/${bindingIndex}/sources`, `${relative}: ${error instanceof Error ? error.message : String(error)}`));
      }
    }
    const evidence = Array.isArray(binding.stepEvidence) ? binding.stepEvidence : [];
    const evidenceIds = evidence.map((item) => item.stepId);
    if (!sameSet(evidenceIds, REQUIRED_SEQUENCE) || new Set(evidenceIds).size !== evidenceIds.length) findings.push(finding("step-evidence-set", `/bindings/${bindingIndex}/stepEvidence`, `${binding.language} must bind every pseudocode step exactly once`));
    for (const [evidenceIndex, item] of evidence.entries()) {
      if (!binding.sources?.includes(item.source)) {
        findings.push(finding("evidence-source-not-declared", `/bindings/${bindingIndex}/stepEvidence/${evidenceIndex}/source`, `${item.source} is not in binding.sources`));
        continue;
      }
      const content = sources.get(item.source);
      if (content === undefined) continue;
      let cursor = -1;
      for (const fragment of item.requiredFragments ?? []) {
        const index = item.ordered ? content.indexOf(fragment, cursor + 1) : content.indexOf(fragment);
        if (index < 0) findings.push(finding("source-witness-missing", `/bindings/${bindingIndex}/stepEvidence/${evidenceIndex}`, `${binding.language} ${item.stepId} missing witness ${JSON.stringify(fragment)} in ${item.source}`));
        else if (item.ordered) cursor = index;
      }
    }
    const combined = [...sources.values()].join("\n");
    for (const fragment of binding.forbiddenFragments ?? []) if (combined.includes(fragment)) findings.push(finding("forbidden-source-fragment", `/bindings/${bindingIndex}/forbiddenFragments`, `${binding.language} source contains forbidden fragment ${JSON.stringify(fragment)}`));
  }
  return {findings, sourceDigests};
}

export function applyMutations(value, mutations) {
  const clone = structuredClone(value);
  for (const mutation of mutations) {
    const segments = mutation.path.split("/").slice(1).map((segment) => segment.replaceAll("~1", "/").replaceAll("~0", "~"));
    let parent = clone;
    for (const segment of segments.slice(0, -1)) parent = parent[Array.isArray(parent) ? Number(segment) : segment];
    const key = segments.at(-1);
    if (mutation.op === "set") parent[Array.isArray(parent) ? Number(key) : key] = structuredClone(mutation.value);
    else if (mutation.op === "delete") Array.isArray(parent) ? parent.splice(Number(key), 1) : delete parent[key];
    else if (mutation.op === "swap") { const [left, right] = mutation.value; [parent[left], parent[right]] = [parent[right], parent[left]]; }
    else throw new Error(`unsupported mutation operation ${mutation.op}`);
  }
  return clone;
}
