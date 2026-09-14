/**
 * Portable runtime contract-drift telemetry envelope.
 *
 * This TypeSpec file and the sibling Draft 2020-12 schema are independently
 * authored peer authorities. Runtime SDKs may project these snake_case wire
 * fields into idiomatic language-local names, but may not redefine the wire
 * vocabulary or use generated output to overwrite either authority.
 */
import "@typespec/json-schema";

namespace Ores.Middleware.RuntimeDrift;

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model ContractDriftEvent {
  schema: "ores.middleware.contract-drift/v1";
  drift_kind:
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
  operation_id?: string;
  declaration?: string;
  language?: string;
  runtime?: string;
  runtime_verdict?: "accepted" | "rejected";
  reference_verdict?: "accepted" | "rejected" | "refused";
}
