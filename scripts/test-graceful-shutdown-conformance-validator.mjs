#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { validateGracefulShutdownConformance } from "./lib/graceful-shutdown-conformance.mjs";

const path = new URL("../contracts/graceful-shutdown/conformance.json", import.meta.url);
const canonical = JSON.parse(await readFile(path, "utf8"));

function clone() {
  return structuredClone(canonical);
}

function rejectsMutation(name, mutate) {
  test(name, () => {
    const candidate = clone();
    mutate(candidate);
    assert.throws(() => validateGracefulShutdownConformance(candidate));
  });
}

test("canonical shutdown corpus is admitted", () => {
  assert.deepEqual(validateGracefulShutdownConformance(clone()), {
    retry: 5,
    lifecycle: 5,
    admission: 3,
    transport: 4,
  });
});

rejectsMutation("1: rejects unknown top-level fields", (candidate) => {
  candidate.unreviewed = true;
});

rejectsMutation("2: rejects unknown constant fields", (candidate) => {
  candidate.constants.unreviewed = 1;
});

rejectsMutation("3: rejects phase vocabulary drift", (candidate) => {
  candidate.constants.phases.push("stopped");
});

rejectsMutation("4: rejects duplicate retry milliseconds", (candidate) => {
  candidate.retryAfterCases[1].milliseconds = candidate.retryAfterCases[0].milliseconds;
});

rejectsMutation("5: rejects incorrect Retry-After rounding", (candidate) => {
  candidate.retryAfterCases[2].delaySeconds = 1;
});

rejectsMutation("6: rejects an unreviewed lifecycle case", (candidate) => {
  candidate.lifecycleCases.push({
    name: "unreviewed-case",
    initialActive: 0,
    action: "drain",
    expectedOutcome: "drained",
    expectedPhase: "draining",
    expectedRemaining: 0,
  });
});

rejectsMutation("7: rejects lifecycle action drift", (candidate) => {
  candidate.lifecycleCases[0].action = "shutdown-listener";
});

rejectsMutation("8: rejects lifecycle remaining-count drift", (candidate) => {
  candidate.lifecycleCases[2].expectedRemaining = 0;
});

rejectsMutation("9: rejects rejection metadata on the running phase", (candidate) => {
  candidate.admissionCases[0].status = 200;
});

rejectsMutation("10: rejects non-canonical generic header casing", (candidate) => {
  candidate.admissionCases[1].genericHeaders = { "Retry-After": "5" };
});

rejectsMutation("11: rejects hop-by-hop Connection in generic metadata", (candidate) => {
  candidate.admissionCases[1].genericHeaders.connection = "close";
});

rejectsMutation("12: rejects unknown transport protocols", (candidate) => {
  candidate.transportCases[3].protocol = "HTTP/4";
});

rejectsMutation("13: rejects duplicate transport names", (candidate) => {
  candidate.transportCases[1].name = candidate.transportCases[0].name;
});

rejectsMutation("14: rejects ownership overlap", (candidate) => {
  candidate.ownership.middlewareOwns.push("signal-policy");
});

rejectsMutation("15: rejects missing resource-close ownership", (candidate) => {
  candidate.ownership.consumerOwns = candidate.ownership.consumerOwns.filter((item) => item !== "resource-close");
});
