import assert from "node:assert/strict";
import test from "node:test";

import {
  CONTRACT_DRIFT_EVENT_SCHEMA,
  TJSV_LANGUAGE_BOUNDARY_EVIDENCE_SCHEMA,
  compareContractArtifactEvidence,
  compareRuntimeContractVerdicts,
  toContractDriftEvent
} from "../dist/contract-drift.js";
import { checkRequestContract } from "../dist/request-contract.js";

const sha = (character) => character.repeat(64);
const revision = (character) => character.repeat(40);

function evidence(overrides = {}) {
  return {
    schema: TJSV_LANGUAGE_BOUNDARY_EVIDENCE_SCHEMA,
    language: "typescript",
    runtime: "node@22",
    status: "passed",
    sourceRevision: revision("a"),
    artifactDigest: `sha256:${sha("b")}`,
    receiptRunId: sha("c"),
    contractIrId: sha("d"),
    toolchain: { name: "typescript", version: "5.9.0" },
    generator: { name: "api-docs", version: "1" },
    validation: { ingress: "passed", egress: "passed" },
    ...overrides
  };
}

const expected = Object.freeze({
  receiptRunId: sha("c"),
  contractIrId: sha("d"),
  sourceRevision: revision("a"),
  language: "typescript",
  runtime: "node@22"
});

test("exact TJSV artifact binding has no drift", () => {
  assert.deepEqual(compareContractArtifactEvidence(expected, evidence()), []);
});

test("artifact binding reports categories without leaking digest values", () => {
  const findings = compareContractArtifactEvidence(expected, evidence({
    contractIrId: sha("e"),
    receiptRunId: sha("f"),
    sourceRevision: revision("b")
  }));
  assert.deepEqual(
    findings.map((finding) => finding.kind).sort(),
    ["contract_ir_id_mismatch", "receipt_run_id_mismatch", "source_revision_mismatch"].sort()
  );
  const encoded = JSON.stringify(findings);
  assert.equal(encoded.includes(sha("c")), false);
  assert.equal(encoded.includes(sha("d")), false);
  assert.equal(encoded.includes(sha("e")), false);
  assert.equal(encoded.includes(sha("f")), false);
});

test("failed or incomplete boundary evidence is visible", () => {
  const findings = compareContractArtifactEvidence(expected, evidence({
    status: "stopped_for_evaluation",
    validation: { ingress: "failed", egress: "failed" }
  }));
  assert.deepEqual(
    findings.map((finding) => finding.kind).sort(),
    ["evidence_not_passed", "ingress_validation_failed", "egress_validation_failed"].sort()
  );
});

test("malformed evidence fails closed as one bounded category", () => {
  const findings = compareContractArtifactEvidence(expected, evidence({
    artifactDigest: "not-a-digest"
  }));
  assert.deepEqual(findings.map((finding) => finding.kind), ["evidence_malformed"]);
  assert.deepEqual(compareContractArtifactEvidence(expected, null).map((finding) => finding.kind), ["evidence_malformed"]);
});

test("ordinary invalid input is not drift when both validators reject it", () => {
  assert.equal(compareRuntimeContractVerdicts({
    runtimeAccepted: false,
    referenceVerdict: "rejected",
    operationId: "items.create",
    declaration: "CreateItemRequest"
  }), undefined);
});

test("runtime/reference disagreement produces payload-free drift metadata", () => {
  const finding = compareRuntimeContractVerdicts({
    runtimeAccepted: true,
    referenceVerdict: "rejected",
    operationId: "items.create",
    declaration: "CreateItemRequest",
    language: "typescript",
    runtime: "node@22"
  });
  assert.equal(finding?.kind, "runtime_verdict_divergence");
  assert.equal(finding?.runtimeVerdict, "accepted");
  assert.equal(finding?.referenceVerdict, "rejected");
  assert.deepEqual(
    Object.keys(finding).sort(),
    ["declaration", "kind", "language", "operationId", "referenceVerdict", "runtime", "runtimeVerdict"].sort()
  );

  const event = toContractDriftEvent(finding);
  assert.deepEqual(event, {
    schema: CONTRACT_DRIFT_EVENT_SCHEMA,
    drift_kind: "runtime_verdict_divergence",
    operation_id: "items.create",
    declaration: "CreateItemRequest",
    language: "typescript",
    runtime: "node@22",
    runtime_verdict: "accepted",
    reference_verdict: "rejected"
  });
  assert.equal("operationId" in event, false);
  assert.equal("runtimeVerdict" in event, false);
});

test("reference refusal is drift evidence, not an invalid-input verdict", () => {
  const finding = compareRuntimeContractVerdicts({
    runtimeAccepted: false,
    referenceVerdict: "refused",
    operationId: "items.create"
  });
  assert.equal(finding?.kind, "reference_validation_refused");
  assert.equal(finding?.runtimeVerdict, "rejected");
});

test("request contract shadow validation observes only disagreement", async () => {
  const observed = [];
  const issue = Object.freeze({ path: "/body/name", code: "required", message: "name is required" });
  const request = new Request("https://example.test/v1/items", { method: "POST" });

  const failure = await checkRequestContract(
    {
      resolve() {
        return {
          pathTemplate: "/v1/items",
          operationId: "items.create",
          declaration: "CreateItemRequest",
          language: "typescript",
          runtime: "node@22",
          validate() { return [issue]; },
          referenceValidate() { return "rejected"; }
        };
      }
    },
    request,
    undefined,
    (finding) => { observed.push(finding); }
  );

  assert.equal(failure?.code, "request_contract_validation_failed");
  assert.equal(observed.length, 0);

  const acceptedRequest = new Request("https://example.test/v1/items", { method: "POST" });
  const accepted = await checkRequestContract(
    {
      resolve() {
        return {
          pathTemplate: "/v1/items",
          operationId: "items.create",
          declaration: "CreateItemRequest",
          language: "typescript",
          runtime: "node@22",
          validate() { return []; },
          referenceValidate() { return "rejected"; }
        };
      }
    },
    acceptedRequest,
    undefined,
    (finding) => { observed.push(finding); }
  );

  assert.equal(accepted, undefined);
  assert.equal(observed.length, 1);
  assert.equal(observed[0].kind, "runtime_verdict_divergence");
});
