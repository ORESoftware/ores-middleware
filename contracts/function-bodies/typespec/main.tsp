import "@typespec/json-schema";

/**
 * Independently authored compile-time authority for critical middleware
 * function/method body plans. JSON Schema is a peer authority, not an emitter
 * target or fallback for this source.
 */
namespace Ores.Middleware.FunctionBodies;

/** Shared identifier shape used by plan and evidence records. */
@pattern("^[A-Za-z][A-Za-z0-9_.:-]{0,127}$")
scalar SafeIdentifier extends string;

/** Relative repository path used by native-source evidence bindings. */
@minLength(1)
@maxLength(256)
@pattern("^(?!/)(?!.*(?:^|/)\\.\\.(?:/|$))[A-Za-z0-9._/-]+$")
scalar RelativePath extends string;

/** A nontrivial native-source witness fragment. */
@minLength(2)
@maxLength(512)
scalar RequiredFragment extends string;

/** A forbidden native-source fragment used by negative evidence. */
@minLength(3)
@maxLength(256)
scalar ForbiddenFragment extends string;

/** Native runtimes required by the reviewed function-body evidence matrix. */
enum BodyLanguage {
  rust: "rust",
  typescript: "typescript",
  golang: "golang",
  gleam: "gleam",
  elixir: "elixir",
  erlang: "erlang",
}

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
@TypeSpec.JsonSchema.extension("allOf", #[
  #{
    `if`: #{
      `properties`: #{ `opcode`: #{ `const`: "report-failure-fail-open" } },
      `required`: #["opcode"]
    },
    `then`: #{
      `properties`: #{
        `when`: #{ `const`: "on-failure" },
        `failurePolicy`: #{ `const`: "ignore-reporter-failure" },
        `effects`: #{ `type`: "array", `contains`: #{ `const`: "emit-bounded-telemetry" } }
      }
    }
  },
  #{
    `if`: #{
      `properties`: #{ `opcode`: #{ `const`: "restore-context" } },
      `required`: #["opcode"]
    },
    `then`: #{
      `properties`: #{
        `phase`: #{ `const`: "finalization" },
        `when`: #{ `const`: "finally" },
        `failurePolicy`: #{ `const`: "always-run" },
        `effects`: #{ `type`: "array", `contains`: #{ `const`: "restore-ambient-context" } }
      }
    }
  },
  #{
    `if`: #{
      `properties`: #{ `opcode`: #{ `const`: "invoke-operation" } },
      `required`: #["opcode"]
    },
    `then`: #{
      `properties`: #{
        `when`: #{ `const`: "unless-failed" },
        `effects`: #{ `type`: "array", `contains`: #{ `const`: "invoke-user-code" } }
      }
    }
  },
  #{
    `if`: #{
      `properties`: #{ `terminal`: #{ `const`: true } },
      `required`: #["terminal"]
    },
    `then`: #{
      `properties`: #{
        `phase`: #{ `const`: "terminal" },
        `opcode`: #{ `const`: "return-typed-outcome" },
        `effects`: #{ `type`: "array", `contains`: #{ `const`: "return-outcome" } }
      }
    }
  }
])
model FunctionBodyStep {
  id: SafeIdentifier;
  opcode: BodyOpcode;
  phase: BodyPhase;
  when: BodyWhen;

  @maxItems(16)
  @TypeSpec.JsonSchema.uniqueItems
  dependsOn: SafeIdentifier[];

  @minItems(1)
  @maxItems(16)
  @TypeSpec.JsonSchema.uniqueItems
  effects: BodyEffect[];

  failurePolicy: BodyFailurePolicy;
  terminal: boolean;
}

/** Public failure fields cannot include any reviewed private field. */
@minItems(1)
@maxItems(32)
@TypeSpec.JsonSchema.uniqueItems
@TypeSpec.JsonSchema.extension("allOf", #[#{
  `not`: #{
    `type`: "array",
    `contains`: #{
      `enum`: #["cause", "message", "stack", "body", "authorization", "cookie", "token", "rawIdentity"]
    }
  }
}])
model PublicFailureFields is Array<SafeIdentifier>;

/** Every reviewed private field must remain explicitly forbidden. */
@minItems(8)
@maxItems(32)
@TypeSpec.JsonSchema.uniqueItems
@TypeSpec.JsonSchema.extension("allOf", #[
  #{ `type`: "array", `contains`: #{ `const`: "cause" } },
  #{ `type`: "array", `contains`: #{ `const`: "message" } },
  #{ `type`: "array", `contains`: #{ `const`: "stack" } },
  #{ `type`: "array", `contains`: #{ `const`: "body" } },
  #{ `type`: "array", `contains`: #{ `const`: "authorization" } },
  #{ `type`: "array", `contains`: #{ `const`: "cookie" } },
  #{ `type`: "array", `contains`: #{ `const`: "token" } },
  #{ `type`: "array", `contains`: #{ `const`: "rawIdentity" } }
])
model ForbiddenPublicFields is Array<SafeIdentifier>;

/**
 * One language-neutral executable pseudocode body. Native implementations may
 * use different syntax, runtimes, or control primitives, but must provide
 * source and runtime evidence for every semantic step.
 */
@TypeSpec.JsonSchema.extension("additionalProperties", false)
model FunctionBodyPlan {
  schemaVersion: "1.0.0";

  @maxLength(128)
  @pattern("^[a-z0-9][a-z0-9._/-]*$")
  contractId: string;

  @maxLength(128)
  @pattern("^[A-Za-z][A-Za-z0-9_.:/-]*$")
  function: string;

  @minLength(20)
  @maxLength(1024)
  intent: string;

  @minItems(1)
  @maxItems(32)
  @TypeSpec.JsonSchema.uniqueItems
  inputs: SafeIdentifier[];

  @minItems(1)
  @maxItems(16)
  @TypeSpec.JsonSchema.uniqueItems
  outputs: SafeIdentifier[];

  publicFailureFields: PublicFailureFields;
  forbiddenPublicFields: ForbiddenPublicFields;

  @minItems(11)
  @maxItems(32)
  steps: FunctionBodyStep[];
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model BodyStepEvidence {
  @maxLength(128)
  @pattern("^[A-Za-z][A-Za-z0-9_.:-]*$")
  stepId: string;

  source: RelativePath;

  @minItems(1)
  @maxItems(8)
  @TypeSpec.JsonSchema.uniqueItems
  requiredFragments: RequiredFragment[];

  ordered: boolean;
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model LanguageBodyBinding {
  @TypeSpec.JsonSchema.extension("enum", #["rust", "typescript", "golang", "gleam", "elixir", "erlang"])
  language: "rust" | "typescript" | "golang" | "gleam" | "elixir" | "erlang";

  @minLength(3)
  @maxLength(256)
  symbol: string;

  @minItems(1)
  @maxItems(4)
  @TypeSpec.JsonSchema.uniqueItems
  sources: RelativePath[];

  @minItems(11)
  @maxItems(11)
  stepEvidence: BodyStepEvidence[];

  @maxItems(32)
  @TypeSpec.JsonSchema.uniqueItems
  forbiddenFragments: ForbiddenFragment[];
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model FunctionBodyBindings {
  schemaVersion: "1.0.0";

  @maxLength(128)
  @pattern("^[a-z0-9][a-z0-9._/-]*$")
  contractId: string;

  @pattern("^contracts/function-bodies/[A-Za-z0-9._/-]+\\.json$")
  plan: string;

  @minItems(6)
  @maxItems(6)
  bindings: LanguageBodyBinding[];
}
