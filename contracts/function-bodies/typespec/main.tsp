/**
 * Independently authored compile-time authority for critical middleware
 * function/method body plans. JSON Schema is a peer authority, not an emitter
 * target or fallback for this source.
 */
namespace Ores.Middleware.FunctionBodies;

/** Stable pseudocode operations whose ordering and effects are executable. */
enum BodyOpcode {
  captureContext: "capture-context",
  normalizeInput: "normalize-input",
  enterContext: "enter-context",
  enterFailureBoundary: "enter-failure-boundary",
  armTerminationGuard: "arm-termination-guard",
  invokeOperation: "invoke-operation",
  classifyFailure: "classify-failure",
  redactPublicFailure: "redact-public-failure",
  reportFailureFailOpen: "report-failure-fail-open",
  restoreContext: "restore-context",
  returnTypedOutcome: "return-typed-outcome",
}

enum BodyPhase {
  setup: "setup",
  guarded: "guarded",
  failure: "failure",
  finalization: "finalization",
  terminal: "terminal",
}

enum BodyWhen {
  always: "always",
  unlessFailed: "unless-failed",
  onFailure: "on-failure",
  finally: "finally",
}

enum BodyEffect {
  readAmbientContext: "read-ambient-context",
  snapshotAllowlistedContext: "snapshot-allowlisted-context",
  normalizeLowCardinalityInput: "normalize-low-cardinality-input",
  installRequestContext: "install-request-context",
  catchLanguageFailure: "catch-language-failure",
  observeCancellationOrDeadline: "observe-cancellation-or-deadline",
  invokeUserCode: "invoke-user-code",
  classifyFailureWithoutPayload: "classify-failure-without-payload",
  constructPublicFailure: "construct-public-failure",
  emitBoundedTelemetry: "emit-bounded-telemetry",
  restoreAmbientContext: "restore-ambient-context",
  returnOutcome: "return-outcome",
}

enum BodyFailurePolicy {
  mapToTypedFailure: "map-to-typed-failure",
  ignoreReporterFailure: "ignore-reporter-failure",
  alwaysRun: "always-run",
  notApplicable: "not-applicable",
}

model FunctionBodyStep {
  id: string;
  opcode: BodyOpcode;
  phase: BodyPhase;
  when: BodyWhen;
  dependsOn: string[];
  effects: BodyEffect[];
  failurePolicy: BodyFailurePolicy;
  terminal: boolean;
}

/**
 * One language-neutral executable pseudocode body. Native implementations may
 * use different syntax, runtimes, or control primitives, but must provide
 * source and runtime evidence for every semantic step.
 */
model FunctionBodyPlan {
  schemaVersion: "1.0.0";
  contractId: string;
  function: string;
  intent: string;
  inputs: string[];
  outputs: string[];
  publicFailureFields: string[];
  forbiddenPublicFields: string[];
  steps: FunctionBodyStep[];
}

model BodyStepEvidence {
  stepId: string;
  source: string;
  requiredFragments: string[];
  ordered: boolean;
}

model LanguageBodyBinding {
  language: "rust" | "typescript" | "golang" | "gleam" | "elixir" | "erlang";
  symbol: string;
  sources: string[];
  stepEvidence: BodyStepEvidence[];
  forbiddenFragments: string[];
}

model FunctionBodyBindings {
  schemaVersion: "1.0.0";
  contractId: string;
  plan: string;
  bindings: LanguageBodyBinding[];
}
