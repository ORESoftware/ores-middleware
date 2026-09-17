import assert from "node:assert/strict";

const TOP_LEVEL_KEYS = [
  "admissionCases",
  "constants",
  "lifecycleCases",
  "ownership",
  "retryAfterCases",
  "schema",
  "transportCases",
];

const CONSTANT_KEYS = [
  "cacheControl",
  "contentType",
  "defaultDrainTimeoutMs",
  "defaultRetryAfterMs",
  "errorCode",
  "httpStatus",
  "phases",
];

const EXPECTED_LIFECYCLE = new Map([
  ["zero-active-drain", {
    initialActive: 0,
    action: "drain",
    expectedOutcome: "drained",
    expectedPhase: "draining",
    expectedRemaining: 0,
  }],
  ["existing-request-completes", {
    initialActive: 1,
    action: "complete-before-deadline",
    expectedOutcome: "drained",
    expectedPhase: "draining",
    expectedRemaining: 0,
  }],
  ["deadline-forces-inflight", {
    initialActive: 1,
    action: "deadline",
    expectedOutcome: "timed_out",
    expectedPhase: "forced",
    expectedRemaining: 1,
  }],
  ["explicit-force", {
    initialActive: 1,
    action: "force",
    expectedOutcome: "forced",
    expectedPhase: "forced",
    expectedRemaining: 1,
  }],
  ["zero-timeout", {
    initialActive: 1,
    action: "zero-timeout-drain",
    expectedOutcome: "timed_out",
    expectedPhase: "forced",
    expectedRemaining: 1,
  }],
]);

const EXPECTED_ADMISSION = new Map([
  ["running", {
    name: "running-admits",
    phase: "running",
    admitted: true,
  }],
  ["draining", {
    name: "draining-rejects",
    phase: "draining",
    admitted: false,
    status: 429,
    errorCode: "service_draining",
    genericHeaders: { "retry-after": "5" },
  }],
  ["forced", {
    name: "forced-rejects",
    phase: "forced",
    admitted: false,
    status: 429,
    errorCode: "service_draining",
    genericHeaders: { "retry-after": "5" },
  }],
]);

const EXPECTED_TRANSPORT = new Map([
  ["HTTP/1.0", { name: "http-1-0-draining", protocol: "HTTP/1.0", connectionClose: true }],
  ["HTTP/1.1", { name: "http-1-1-draining", protocol: "HTTP/1.1", connectionClose: true }],
  ["HTTP/2", { name: "http-2-draining", protocol: "HTTP/2", connectionClose: false }],
  ["HTTP/3", { name: "http-3-draining", protocol: "HTTP/3", connectionClose: false }],
]);

const EXPECTED_MIDDLEWARE_OWNERSHIP = [
  "application-admission",
  "in-flight-accounting",
  "canonical-rejection",
];

const EXPECTED_CONSUMER_OWNERSHIP = [
  "signal-policy",
  "tty-policy",
  "listener-shutdown",
  "transport-drain",
  "telemetry-flush",
  "resource-close",
  "process-exit",
];

function assertPlainObject(value, label) {
  assert(value !== null && typeof value === "object" && !Array.isArray(value), `${label} must be an object`);
}

function assertExactKeys(value, expectedKeys, label) {
  assertPlainObject(value, label);
  assert.deepEqual(Object.keys(value).sort(), [...expectedKeys].sort(), `${label} keys drifted`);
}

function assertUniqueStrings(values, label) {
  assert(Array.isArray(values) && values.length > 0, `${label} must be a non-empty array`);
  for (const value of values) {
    assert.equal(typeof value, "string", `${label} values must be strings`);
    assert(value.length > 0, `${label} values must be non-empty`);
  }
  assert.equal(new Set(values).size, values.length, `${label} must not contain duplicates`);
}

export function validateGracefulShutdownConformance(contract) {
  assertExactKeys(contract, TOP_LEVEL_KEYS, "contract");
  assert.equal(contract.schema, "ores.middleware.graceful-shutdown-conformance/v1");

  assertExactKeys(contract.constants, CONSTANT_KEYS, "constants");
  assert.deepEqual(contract.constants.phases, ["running", "draining", "forced"]);
  assert.equal(contract.constants.defaultDrainTimeoutMs, 5_000);
  assert.equal(contract.constants.defaultRetryAfterMs, 5_000);
  assert.equal(contract.constants.httpStatus, 429);
  assert.equal(contract.constants.errorCode, "service_draining");
  assert.equal(contract.constants.contentType, "application/problem+json");
  assert.equal(contract.constants.cacheControl, "no-store");

  assert(Array.isArray(contract.retryAfterCases) && contract.retryAfterCases.length > 0, "retryAfterCases must be non-empty");
  let previousMilliseconds = 0;
  const seenRetryMilliseconds = new Set();
  for (const [index, vector] of contract.retryAfterCases.entries()) {
    assertExactKeys(vector, ["milliseconds", "delaySeconds"], `retryAfterCases[${index}]`);
    assert(Number.isSafeInteger(vector.milliseconds) && vector.milliseconds > 0, "retry milliseconds must be a positive safe integer");
    assert(Number.isSafeInteger(vector.delaySeconds) && vector.delaySeconds > 0, "retry seconds must be a positive safe integer");
    assert(!seenRetryMilliseconds.has(vector.milliseconds), `duplicate retry vector ${vector.milliseconds}ms`);
    assert(vector.milliseconds > previousMilliseconds, "retry vectors must be strictly increasing by milliseconds");
    seenRetryMilliseconds.add(vector.milliseconds);
    previousMilliseconds = vector.milliseconds;
    assert.equal(
      vector.delaySeconds,
      Math.max(1, Math.ceil(vector.milliseconds / 1_000)),
      `retry-after rounding drift for ${vector.milliseconds}ms`,
    );
  }

  assert.equal(contract.lifecycleCases.length, EXPECTED_LIFECYCLE.size, "lifecycle case set drifted");
  const lifecycleNames = new Set();
  for (const [index, vector] of contract.lifecycleCases.entries()) {
    assertPlainObject(vector, `lifecycleCases[${index}]`);
    assert.equal(typeof vector.name, "string", "lifecycle case name must be a string");
    assert(!lifecycleNames.has(vector.name), `duplicate lifecycle case ${vector.name}`);
    lifecycleNames.add(vector.name);
    const expected = EXPECTED_LIFECYCLE.get(vector.name);
    assert(expected, `unsupported lifecycle case ${vector.name}`);
    assert.deepEqual(vector, { name: vector.name, ...expected }, `lifecycle case drifted: ${vector.name}`);
  }
  assert.deepEqual([...lifecycleNames].sort(), [...EXPECTED_LIFECYCLE.keys()].sort(), "lifecycle case names drifted");

  assert.equal(contract.admissionCases.length, EXPECTED_ADMISSION.size, "admission case set drifted");
  const admissionByPhase = new Map();
  for (const [index, vector] of contract.admissionCases.entries()) {
    assertPlainObject(vector, `admissionCases[${index}]`);
    assert(contract.constants.phases.includes(vector.phase), `unsupported admission phase ${vector.phase}`);
    assert(!admissionByPhase.has(vector.phase), `duplicate admission phase ${vector.phase}`);
    admissionByPhase.set(vector.phase, vector);
    assert.deepEqual(vector, EXPECTED_ADMISSION.get(vector.phase), `admission case drifted: ${vector.phase}`);
    if (vector.genericHeaders) {
      for (const name of Object.keys(vector.genericHeaders)) {
        assert.equal(name, name.toLowerCase(), `generic header must be lowercase: ${name}`);
        assert.notEqual(name, "connection", "generic rejection must not own hop-by-hop Connection");
      }
    }
  }
  assert.deepEqual([...admissionByPhase.keys()].sort(), [...EXPECTED_ADMISSION.keys()].sort(), "admission phases drifted");

  assert.equal(contract.transportCases.length, EXPECTED_TRANSPORT.size, "transport case set drifted");
  const transportByProtocol = new Map();
  const transportNames = new Set();
  for (const [index, vector] of contract.transportCases.entries()) {
    assertExactKeys(vector, ["name", "protocol", "connectionClose"], `transportCases[${index}]`);
    assert(!transportByProtocol.has(vector.protocol), `duplicate transport protocol ${vector.protocol}`);
    assert(!transportNames.has(vector.name), `duplicate transport name ${vector.name}`);
    transportByProtocol.set(vector.protocol, vector);
    transportNames.add(vector.name);
    const expected = EXPECTED_TRANSPORT.get(vector.protocol);
    assert(expected, `unsupported transport protocol ${vector.protocol}`);
    assert.deepEqual(vector, expected, `transport case drifted: ${vector.protocol}`);
  }
  assert.deepEqual([...transportByProtocol.keys()].sort(), [...EXPECTED_TRANSPORT.keys()].sort(), "transport protocols drifted");

  assertExactKeys(contract.ownership, ["middlewareOwns", "consumerOwns"], "ownership");
  assertUniqueStrings(contract.ownership.middlewareOwns, "ownership.middlewareOwns");
  assertUniqueStrings(contract.ownership.consumerOwns, "ownership.consumerOwns");
  assert.deepEqual(contract.ownership.middlewareOwns, EXPECTED_MIDDLEWARE_OWNERSHIP, "middleware ownership drifted");
  assert.deepEqual(contract.ownership.consumerOwns, EXPECTED_CONSUMER_OWNERSHIP, "consumer ownership drifted");

  const middlewareOwns = new Set(contract.ownership.middlewareOwns);
  const consumerOwns = new Set(contract.ownership.consumerOwns);
  for (const boundary of middlewareOwns) {
    assert(!consumerOwns.has(boundary), `ownership overlap: ${boundary}`);
  }

  return {
    retry: contract.retryAfterCases.length,
    lifecycle: contract.lifecycleCases.length,
    admission: contract.admissionCases.length,
    transport: contract.transportCases.length,
  };
}
