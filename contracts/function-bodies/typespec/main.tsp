import "@typespec/json-schema";

/**
 * Independently authored compile-time authority for critical middleware
 * function/method body plans. JSON Schema is a peer authority, not an emitter
 * target or fallback for this source.
 */
namespace Ores.Middleware.FunctionBodies;

/** Shared identifier shape used by plan and evidence records. */
scalar SafeIdentifier extends string;

/** Relative repository path used by native-source evidence bindings. */
scalar RelativePath extends string;

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

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model FunctionBodyStep {
  id: SafeIdentifier;
  opcode: BodyOpcode;
  phase: BodyPhase;
  when: BodyWhen;
  dependsOn: SafeIdentifier[];
  effects: BodyEffect[];
  failurePolicy: BodyFailurePolicy;
  terminal: boolean;
}

/**
 * One language-neutral executable pseudocode body. Native implementations may
 * use different syntax, runtimes, or control primitives, but must provide
 * source and runtime evidence for every semantic step.
 */
@TypeSpec.JsonSchema.extension("additionalProperties", false)
model FunctionBodyPlan {
  schemaVersion: "1.0.0";
  contractId: string;
  function: string;
  intent: string;
  inputs: SafeIdentifier[];
  outputs: SafeIdentifier[];
  publicFailureFields: SafeIdentifier[];
  forbiddenPublicFields: SafeIdentifier[];
  steps: FunctionBodyStep[];
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model BodyStepEvidence {
  stepId: SafeIdentifier;
  source: RelativePath;
  requiredFragments: string[];
  ordered: boolean;
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model LanguageBodyBinding {
  language: "rust" | "typescript" | "golang" | "gleam" | "elixir" | "erlang";
  symbol: string;
  sources: RelativePath[];
  stepEvidence: BodyStepEvidence[];
  forbiddenFragments: string[];
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model FunctionBodyBindings {
  schemaVersion: "1.0.0";
  contractId: string;
  plan: string;
  bindings: LanguageBodyBinding[];
}
