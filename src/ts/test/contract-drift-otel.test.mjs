import assert from "node:assert/strict";
import test from "node:test";

import {
  createOresContractDriftObserver,
  withOresContractDriftLogging
} from "../dist/otel.js";

function recordingLogger(records) {
  return {
    warn(message) {
      let fields = {};
      return {
        addFields(nextFields) {
          fields = { ...fields, ...nextFields };
          return this;
        },
        send() {
          records.push({ message, fields });
          return Promise.resolve();
        }
      };
    }
  };
}

test("ores-otel drift event is bounded and excludes receipt digests", () => {
  const records = [];
  const observer = createOresContractDriftObserver(recordingLogger(records));
  observer({
    kind: "runtime_verdict_divergence",
    operationId: "items.create",
    declaration: "CreateItemRequest",
    language: "typescript",
    runtime: "node@22",
    runtimeVerdict: "accepted",
    referenceVerdict: "rejected"
  });

  assert.equal(records.length, 1);
  assert.equal(records[0].message, "contract runtime drift");
  assert.deepEqual(records[0].fields, {
    "event.name": "ores.contract.drift",
    "contract.drift_schema": "ores.middleware.contract-drift/v1",
    "contract.drift_kind": "runtime_verdict_divergence",
    "contract.operation_id": "items.create",
    "contract.declaration": "CreateItemRequest",
    "contract.language": "typescript",
    "contract.runtime": "node@22",
    "contract.runtime_verdict": "accepted",
    "contract.reference_verdict": "rejected"
  });
  assert.equal("request.body" in records[0].fields, false);
  assert.equal("contract.receipt_run_id" in records[0].fields, false);
  assert.equal("contract.contract_ir_id" in records[0].fields, false);
});

test("ores-otel wrapper preserves validator resolution and adds a drift observer", async () => {
  const records = [];
  const validator = {
    resolve(method, pathname) {
      return { pathTemplate: `${method}:${pathname}`, validate() { return []; } };
    }
  };
  const wrapped = withOresContractDriftLogging(validator, recordingLogger(records));
  assert.equal(typeof wrapped.driftObserver, "function");
  assert.equal((await wrapped.resolve("GET", "/health")).pathTemplate, "GET:/health");

  wrapped.driftObserver({ kind: "reference_validation_refused", operationId: "health.get" });
  assert.equal(records.length, 1);
  assert.equal(records[0].fields["contract.drift_schema"], "ores.middleware.contract-drift/v1");
  assert.equal(records[0].fields["contract.drift_kind"], "reference_validation_refused");
});

test("explicit consumer drift observer is not replaced", () => {
  const explicit = () => undefined;
  const validator = { driftObserver: explicit, resolve() { return undefined; } };
  const wrapped = withOresContractDriftLogging(validator, recordingLogger([]));
  assert.equal(wrapped, validator);
  assert.equal(wrapped.driftObserver, explicit);
});
