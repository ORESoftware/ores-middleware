import assert from "node:assert/strict";
import test from "node:test";

import {
  DEFAULT_DRAIN_TIMEOUT_MS,
  DEFAULT_RETRY_AFTER_MS,
  MAX_TIMER_DELAY_MS,
  SHUTDOWN_HTTP_STATUS,
  ShutdownCoordinator
} from "../dist/shutdown.js";

test("defaults match the canonical five-second drain policy", () => {
  assert.equal(DEFAULT_DRAIN_TIMEOUT_MS, 5_000);
  assert.equal(DEFAULT_RETRY_AFTER_MS, 5_000);
  assert.equal(SHUTDOWN_HTTP_STATUS, 429);
});

test("request leases account exactly once", () => {
  const coordinator = new ShutdownCoordinator();
  const first = coordinator.tryBeginRequest();
  const second = coordinator.tryBeginRequest();
  assert.equal(first.ok, true);
  assert.equal(second.ok, true);
  assert.equal(coordinator.activeRequests, 2);

  first.lease.finish();
  first.lease.finish();
  assert.equal(coordinator.activeRequests, 1);
  second.lease.finish();
  assert.equal(coordinator.activeRequests, 0);
});

test("draining rejects new work with bounded end-to-end retry metadata", () => {
  const coordinator = new ShutdownCoordinator(5_000, 1_501);
  const existing = coordinator.tryBeginRequest();
  assert.equal(existing.ok, true);
  assert.equal(coordinator.startDraining(), true);
  assert.equal(coordinator.startDraining(), false);

  const rejected = coordinator.tryBeginRequest();
  assert.equal(rejected.ok, false);
  assert.deepEqual(rejected.rejection, {
    status: 429,
    code: "service_draining",
    message: "service is draining and is not accepting new requests",
    headers: {
      "retry-after": "2"
    }
  });
  assert.equal(Object.isFrozen(rejected.rejection.headers), true);
  assert.throws(() => {
    rejected.rejection.headers["retry-after"] = "99";
  }, TypeError);
  assert.equal(coordinator.activeRequests, 1);
  existing.lease.finish();
});

test("sub-second retry hints round up to one second", () => {
  const coordinator = new ShutdownCoordinator(5_000, 1);
  assert.equal(coordinator.rejection().headers["retry-after"], "1");
});

test("exact-second retry hints remain exact", () => {
  const coordinator = new ShutdownCoordinator(5_000, 2_000);
  assert.equal(coordinator.rejection().headers["retry-after"], "2");
});

test("drain waits for already-admitted requests", async () => {
  const coordinator = new ShutdownCoordinator();
  const admission = coordinator.tryBeginRequest();
  assert.equal(admission.ok, true);

  const draining = coordinator.drain();
  assert.equal(coordinator.phase, "draining");
  assert.equal(coordinator.isAcceptingRequests, false);
  queueMicrotask(() => admission.lease.finish());

  assert.deepEqual(await draining, { kind: "drained", active_at_start: 1 });
  assert.equal(coordinator.activeRequests, 0);
});

test("drain without active requests completes immediately", async () => {
  const coordinator = new ShutdownCoordinator();
  assert.deepEqual(await coordinator.drain(), {
    kind: "drained",
    active_at_start: 0
  });
  assert.equal(coordinator.phase, "draining");
  assert.equal(coordinator.isAcceptingRequests, false);
  assert.equal(coordinator.tryBeginRequest().ok, false);
});

test("zero timeout forces in-flight work immediately", async () => {
  const coordinator = new ShutdownCoordinator(0);
  const admission = coordinator.tryBeginRequest();
  assert.equal(admission.ok, true);

  assert.deepEqual(await coordinator.drain(), { kind: "timed_out", remaining: 1 });
  assert.equal(coordinator.phase, "forced");
  assert.equal(coordinator.isForced, true);
  assert.equal(coordinator.tryBeginRequest().ok, false);
  admission.lease.finish();
  assert.equal(coordinator.activeRequests, 0);
});

test("explicit force interrupts a drain", async () => {
  const coordinator = new ShutdownCoordinator(60_000);
  const admission = coordinator.tryBeginRequest();
  assert.equal(admission.ok, true);

  const draining = coordinator.drain();
  assert.equal(coordinator.forceShutdown(), true);
  assert.equal(coordinator.forceShutdown(), false);
  assert.equal(coordinator.isForced, true);
  assert.deepEqual(await draining, { kind: "forced", remaining: 1 });
  admission.lease.finish();
});

test("force from running rejects new work and remains forced through drain", async () => {
  const coordinator = new ShutdownCoordinator(60_000);
  const admission = coordinator.tryBeginRequest();
  assert.equal(admission.ok, true);

  assert.equal(coordinator.forceShutdown(), true);
  assert.equal(coordinator.phase, "forced");
  assert.equal(coordinator.tryBeginRequest().ok, false);
  assert.deepEqual(await coordinator.drain(), { kind: "forced", remaining: 1 });
  admission.lease.finish();
});

test("force before any work yields a zero-remaining forced drain", async () => {
  const coordinator = new ShutdownCoordinator();
  assert.equal(coordinator.forceShutdown(), true);
  assert.deepEqual(await coordinator.drain(), { kind: "forced", remaining: 0 });
  assert.equal(coordinator.activeRequests, 0);
});

test("invalid durations fail closed", () => {
  assert.throws(() => new ShutdownCoordinator(-1), RangeError);
  assert.throws(() => new ShutdownCoordinator(Number.NaN), RangeError);
  assert.throws(() => new ShutdownCoordinator(Number.POSITIVE_INFINITY), RangeError);
  assert.throws(() => new ShutdownCoordinator(1.5), RangeError);
  assert.throws(() => new ShutdownCoordinator(5_000, 0), RangeError);
  assert.throws(() => new ShutdownCoordinator(MAX_TIMER_DELAY_MS + 1), RangeError);
  assert.throws(() => new ShutdownCoordinator(Number.MAX_SAFE_INTEGER + 1), RangeError);
});
