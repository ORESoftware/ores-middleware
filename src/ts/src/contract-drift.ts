export const TJSV_LANGUAGE_BOUNDARY_EVIDENCE_SCHEMA =
  "ores.typespec-json-schema-validator.language-boundary-evidence/v1" as const;
export const CONTRACT_DRIFT_EVENT_SCHEMA =
  "ores.middleware.contract-drift/v1" as const;

const SHA256_RE = /^[a-f0-9]{64}$/;
const SHA256_PREFIXED_RE = /^sha256:[a-f0-9]{64}$/;
const REVISION_RE = /^[a-f0-9]{40}$/;

export type ContractRuntimeVerdict = "accepted" | "rejected" | "refused";

export type ContractDriftKind =
  | "evidence_malformed"
  | "evidence_not_passed"
  | "ingress_validation_failed"
  | "egress_validation_failed"
  | "receipt_run_id_mismatch"
  | "contract_ir_id_mismatch"
  | "source_revision_mismatch"
  | "language_mismatch"
  | "runtime_mismatch"
  | "runtime_verdict_divergence"
  | "reference_validation_refused";

export interface TjsvToolIdentity {
  readonly name: string;
  readonly version: string;
}

/**
 * Runtime-safe projection of the public TJSV language-boundary evidence contract.
 * The full object is produced at build/promotion time; runtime only compares
 * immutable identities and verdicts. It never needs the TypeSpec compiler.
 */
export interface TjsvLanguageBoundaryEvidence {
  readonly schema: typeof TJSV_LANGUAGE_BOUNDARY_EVIDENCE_SCHEMA;
  readonly language: string;
  readonly runtime: string;
  readonly status: "passed" | "failed" | "stopped_for_evaluation";
  readonly sourceRevision: string;
  readonly artifactDigest: string;
  readonly receiptRunId: string;
  readonly contractIrId: string;
  readonly toolchain: TjsvToolIdentity;
  readonly generator: TjsvToolIdentity;
  readonly validation: {
    readonly ingress: "passed" | "failed";
    readonly egress: "passed" | "failed";
  };
}

export interface ExpectedContractBinding {
  readonly receiptRunId: string;
  readonly contractIrId: string;
  readonly sourceRevision?: string;
  readonly language?: string;
  readonly runtime?: string;
}

/**
 * Deliberately bounded language-local drift metadata. Raw payloads, headers,
 * schema fragments, credentials, user IDs, and digest values are excluded so
 * the same object is safe to hand to telemetry sinks.
 */
export interface ContractDriftFinding {
  readonly kind: ContractDriftKind;
  readonly operationId?: string;
  readonly declaration?: string;
  readonly language?: string;
  readonly runtime?: string;
  readonly runtimeVerdict?: Exclude<ContractRuntimeVerdict, "refused">;
  readonly referenceVerdict?: ContractRuntimeVerdict;
}

/** Exact snake_case wire projection admitted by the peer TypeSpec/JSON Schema contract. */
export interface ContractDriftEvent {
  readonly schema: typeof CONTRACT_DRIFT_EVENT_SCHEMA;
  readonly drift_kind: ContractDriftKind;
  readonly operation_id?: string;
  readonly declaration?: string;
  readonly language?: string;
  readonly runtime?: string;
  readonly runtime_verdict?: Exclude<ContractRuntimeVerdict, "refused">;
  readonly reference_verdict?: ContractRuntimeVerdict;
}

export type ContractDriftObserver = (
  finding: ContractDriftFinding
) => void | Promise<void>;

function freezeFinding(
  finding: ContractDriftFinding
): Readonly<ContractDriftFinding> {
  return Object.freeze({ ...finding });
}

function object(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function validToken(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= 256 && value.trim() === value;
}

function validTool(value: unknown): value is TjsvToolIdentity {
  if (!object(value)) return false;
  return validToken(value.name) && validToken(value.version);
}

function evidenceIsWellFormed(
  value: unknown
): value is TjsvLanguageBoundaryEvidence {
  if (!object(value) || !object(value.validation)) return false;
  return value.schema === TJSV_LANGUAGE_BOUNDARY_EVIDENCE_SCHEMA &&
    validToken(value.language) &&
    validToken(value.runtime) &&
    (value.status === "passed" ||
      value.status === "failed" ||
      value.status === "stopped_for_evaluation") &&
    typeof value.sourceRevision === "string" && REVISION_RE.test(value.sourceRevision) &&
    typeof value.artifactDigest === "string" && SHA256_PREFIXED_RE.test(value.artifactDigest) &&
    typeof value.receiptRunId === "string" && SHA256_RE.test(value.receiptRunId) &&
    typeof value.contractIrId === "string" && SHA256_RE.test(value.contractIrId) &&
    validTool(value.toolchain) &&
    validTool(value.generator) &&
    (value.validation.ingress === "passed" || value.validation.ingress === "failed") &&
    (value.validation.egress === "passed" || value.validation.egress === "failed");
}

/** Convert an idiomatic TypeScript finding into the exact shared wire contract. */
export function toContractDriftEvent(
  finding: ContractDriftFinding
): Readonly<ContractDriftEvent> {
  return Object.freeze({
    schema: CONTRACT_DRIFT_EVENT_SCHEMA,
    drift_kind: finding.kind,
    ...(finding.operationId !== undefined ? { operation_id: finding.operationId } : {}),
    ...(finding.declaration !== undefined ? { declaration: finding.declaration } : {}),
    ...(finding.language !== undefined ? { language: finding.language } : {}),
    ...(finding.runtime !== undefined ? { runtime: finding.runtime } : {}),
    ...(finding.runtimeVerdict !== undefined ? { runtime_verdict: finding.runtimeVerdict } : {}),
    ...(finding.referenceVerdict !== undefined ? { reference_verdict: finding.referenceVerdict } : {})
  });
}

/**
 * O(1) startup/registration guard for one generated runtime artifact. The
 * comparison intentionally emits only drift categories, never the digest
 * values themselves. Full evidence remains in the build/promotion receipt.
 */
export function compareContractArtifactEvidence(
  expected: ExpectedContractBinding,
  evidence: unknown
): readonly Readonly<ContractDriftFinding>[] {
  const findings: Readonly<ContractDriftFinding>[] = [];
  const candidate = object(evidence) ? evidence : undefined;
  const candidateLanguage = validToken(candidate?.language) ? candidate.language : expected.language;
  const candidateRuntime = validToken(candidate?.runtime) ? candidate.runtime : expected.runtime;
  const add = (kind: ContractDriftKind): void => {
    findings.push(freezeFinding({
      kind,
      language: candidateLanguage,
      runtime: candidateRuntime
    }));
  };

  if (!SHA256_RE.test(expected.receiptRunId) ||
      !SHA256_RE.test(expected.contractIrId) ||
      (expected.sourceRevision !== undefined && !REVISION_RE.test(expected.sourceRevision)) ||
      !evidenceIsWellFormed(evidence)) {
    add("evidence_malformed");
    return Object.freeze(findings);
  }

  if (evidence.status !== "passed") add("evidence_not_passed");
  if (evidence.validation.ingress !== "passed") add("ingress_validation_failed");
  if (evidence.validation.egress !== "passed") add("egress_validation_failed");
  if (evidence.receiptRunId !== expected.receiptRunId) add("receipt_run_id_mismatch");
  if (evidence.contractIrId !== expected.contractIrId) add("contract_ir_id_mismatch");
  if (expected.sourceRevision !== undefined && evidence.sourceRevision !== expected.sourceRevision) {
    add("source_revision_mismatch");
  }
  if (expected.language !== undefined && evidence.language !== expected.language) add("language_mismatch");
  if (expected.runtime !== undefined && evidence.runtime !== expected.runtime) add("runtime_mismatch");

  return Object.freeze(findings);
}

/**
 * Compare the consumer/runtime validator with a parity-approved reference lane.
 * An ordinary invalid request is not drift when both validators reject it.
 */
export function compareRuntimeContractVerdicts(input: {
  readonly runtimeAccepted: boolean;
  readonly referenceVerdict: ContractRuntimeVerdict;
  readonly operationId?: string;
  readonly declaration?: string;
  readonly language?: string;
  readonly runtime?: string;
}): Readonly<ContractDriftFinding> | undefined {
  const base = {
    operationId: input.operationId,
    declaration: input.declaration,
    language: input.language,
    runtime: input.runtime,
    runtimeVerdict: input.runtimeAccepted ? "accepted" as const : "rejected" as const,
    referenceVerdict: input.referenceVerdict
  };

  if (input.referenceVerdict === "refused") {
    return freezeFinding({ kind: "reference_validation_refused", ...base });
  }

  const referenceAccepted = input.referenceVerdict === "accepted";
  if (input.runtimeAccepted === referenceAccepted) return undefined;
  return freezeFinding({ kind: "runtime_verdict_divergence", ...base });
}

/**
 * Telemetry is observational: a failing sink must never alter request outcome.
 */
export function observeContractDrift(
  observer: ContractDriftObserver | undefined,
  finding: ContractDriftFinding | undefined
): void {
  if (!observer || !finding) return;
  try {
    const result = observer(finding);
    if (result && typeof (result as Promise<void>).catch === "function") {
      void (result as Promise<void>).catch(() => undefined);
    }
  } catch {
    // Drift delivery failure is deliberately detached from request admission.
  }
}

/** Compare one artifact binding and emit each bounded drift category. */
export function observeContractArtifactEvidence(
  expected: ExpectedContractBinding,
  evidence: unknown,
  observer?: ContractDriftObserver
): readonly Readonly<ContractDriftFinding>[] {
  const findings = compareContractArtifactEvidence(expected, evidence);
  for (const finding of findings) observeContractDrift(observer, finding);
  return findings;
}
