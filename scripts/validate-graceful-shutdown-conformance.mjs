#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const path = new URL("../contracts/graceful-shutdown/conformance.json", import.meta.url);
const contract = JSON.parse(await readFile(path, "utf8"));

assert.equal(contract.schema, "ores.middleware.graceful-shutdown-conformance/v1");
assert.deepEqual(contract.constants.phases, ["running", "draining", "forced"]);
assert.equal(contract.constants.defaultDrainTimeoutMs, 5_000);
assert.equal(contract.constants.defaultRetryAfterMs, 5_000);
assert.equal(contract.constants.httpStatus, 429);
assert.equal(contract.constants.errorCode, "service_draining");
assert.equal(contract.constants.contentType, "application/problem+json");
assert.equal(contract.constants.cacheControl, "no-store");

for (const vector of contract.retryAfterCases) {
  assert(Number.isSafeInteger(vector.milliseconds) && vector.milliseconds > 0);
  assert(Number.isSafeInteger(vector.delaySeconds) && vector.delaySeconds > 0);
  assert.equal(
    vector.delaySeconds,
    Math.max(1, Math.ceil(vector.milliseconds / 1_000)),
    `retry-after rounding drift for ${vector.milliseconds}ms`,
  );
}

const lifecycleNames = new Set();
for (const vector of contract.lifecycleCases) {
  assert(!lifecycleNames.has(vector.name), `duplicate lifecycle case ${vector.name}`);
  lifecycleNames.add(vector.name);
  assert(Number.isSafeInteger(vector.initialActive) && vector.initialActive >= 0);
  assert(["drained", "timed_out", "forced"].includes(vector.expectedOutcome));
  assert(contract.constants.phases.includes(vector.expectedPhase));
  assert(Number.isSafeInteger(vector.expectedRemaining) && vector.expectedRemaining >= 0);
  if (vector.expectedOutcome === "timed_out" || vector.expectedOutcome === "forced") {
    assert.equal(vector.expectedPhase, "forced");
  }
  if (vector.expectedOutcome === "drained") {
    assert.equal(vector.expectedRemaining, 0);
  }
}
assert(lifecycleNames.has("zero-active-drain"));
assert(lifecycleNames.has("existing-request-completes"));
assert(lifecycleNames.has("deadline-forces-inflight"));
assert(lifecycleNames.has("explicit-force"));
assert(lifecycleNames.has("zero-timeout"));

const admissionByPhase = new Map();
for (const vector of contract.admissionCases) {
  assert(contract.constants.phases.includes(vector.phase));
  assert(!admissionByPhase.has(vector.phase), `duplicate admission phase ${vector.phase}`);
  admissionByPhase.set(vector.phase, vector);
  if (vector.phase === "running") {
    assert.equal(vector.admitted, true);
    assert.equal(vector.status, undefined);
    continue;
  }
  assert.equal(vector.admitted, false);
  assert.equal(vector.status, contract.constants.httpStatus);
  assert.equal(vector.errorCode, contract.constants.errorCode);
  assert.deepEqual(vector.genericHeaders, { "retry-after": "5" });
  for (const name of Object.keys(vector.genericHeaders)) {
    assert.equal(name, name.toLowerCase(), `generic header must be lowercase: ${name}`);
    assert.notEqual(name, "connection", "generic rejection must not own hop-by-hop Connection");
  }
}
assert.equal(admissionByPhase.size, 3);

const transportByProtocol = new Map();
for (const vector of contract.transportCases) {
  assert(!transportByProtocol.has(vector.protocol), `duplicate protocol ${vector.protocol}`);
  transportByProtocol.set(vector.protocol, vector);
  if (vector.protocol === "HTTP/1.0" || vector.protocol === "HTTP/1.1") {
    assert.equal(vector.connectionClose, true);
  } else if (vector.protocol === "HTTP/2" || vector.protocol === "HTTP/3") {
    assert.equal(vector.connectionClose, false);
  } else {
    assert.fail(`unsupported protocol vector ${vector.protocol}`);
  }
}
assert.equal(transportByProtocol.size, 4);

const middlewareOwns = new Set(contract.ownership.middlewareOwns);
const consumerOwns = new Set(contract.ownership.consumerOwns);
for (const boundary of middlewareOwns) {
  assert(!consumerOwns.has(boundary), `ownership overlap: ${boundary}`);
}
for (const required of ["signal-policy", "tty-policy", "listener-shutdown", "transport-drain", "telemetry-flush", "process-exit"]) {
  assert(consumerOwns.has(required), `missing consumer ownership boundary: ${required}`);
}
for (const required of ["application-admission", "in-flight-accounting", "canonical-rejection"]) {
  assert(middlewareOwns.has(required), `missing middleware ownership boundary: ${required}`);
}

console.log(
  `graceful-shutdown-conformance: ${contract.retryAfterCases.length} retry, ${contract.lifecycleCases.length} lifecycle, ${contract.admissionCases.length} admission, and ${contract.transportCases.length} transport vectors passed`,
);
