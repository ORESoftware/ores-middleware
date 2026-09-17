import assert from "node:assert/strict";
import test from "node:test";

import { ShutdownCoordinator } from "../dist/shutdown.js";

test("many request leases can finish concurrently while drain waits", async () => {
  const coordinator = new ShutdownCoordinator(2_000);
  const admissions = Array.from({ length: 64 }, () => coordinator.tryBeginRequest());
  assert.equal(admissions.every((admission) => admission.ok), true);
  assert.equal(coordinator.activeRequests, 64);

  const draining = coordinator.drain();
  await Promise.all(
    admissions.map((admission) =>
      Promise.resolve().then(() => admission.lease.finish())
    )
  );

  assert.deepEqual(await draining, { kind: "drained", active_at_start: 64 });
  assert.equal(coordinator.activeRequests, 0);
  assert.equal(coordinator.phase, "draining");
});

test("multiple drain callers are released by the same final lease", async () => {
  const coordinator = new ShutdownCoordinator(2_000);
  const admission = coordinator.tryBeginRequest();
  assert.equal(admission.ok, true);

  const first = coordinator.drain();
  const second = coordinator.drain();
  queueMicrotask(() => admission.lease.finish());

  const outcomes = await Promise.all([first, second]);
  assert.deepEqual(outcomes, [
    { kind: "drained", active_at_start: 1 },
    { kind: "drained", active_at_start: 1 }
  ]);
  assert.equal(coordinator.activeRequests, 0);
});

test("out-of-order and repeated lease completion cannot underflow accounting", () => {
  const coordinator = new ShutdownCoordinator();
  const first = coordinator.tryBeginRequest();
  const second = coordinator.tryBeginRequest();
  const third = coordinator.tryBeginRequest();
  assert.equal(first.ok && second.ok && third.ok, true);
  assert.equal(coordinator.activeRequests, 3);

  second.lease.finish();
  second.lease.finish();
  assert.equal(coordinator.activeRequests, 2);
  first.lease.finish();
  assert.equal(coordinator.activeRequests, 1);
  third.lease.finish();
  third.lease.finish();
  assert.equal(coordinator.activeRequests, 0);
});

test("admission stays closed after all pre-drain work has finished", async () => {
  const coordinator = new ShutdownCoordinator(2_000);
  const admission = coordinator.tryBeginRequest();
  assert.equal(admission.ok, true);

  const draining = coordinator.drain();
  queueMicrotask(() => admission.lease.finish());
  assert.deepEqual(await draining, { kind: "drained", active_at_start: 1 });

  assert.equal(coordinator.phase, "draining");
  assert.equal(coordinator.tryBeginRequest().ok, false);
});

test("forced phase remains monotonic after all admitted work is released", async () => {
  const coordinator = new ShutdownCoordinator(60_000);
  const first = coordinator.tryBeginRequest();
  const second = coordinator.tryBeginRequest();
  assert.equal(first.ok && second.ok, true);

  assert.equal(coordinator.forceShutdown(), true);
  first.lease.finish();
  second.lease.finish();
  assert.equal(coordinator.activeRequests, 0);
  assert.equal(coordinator.phase, "forced");
  assert.equal(coordinator.startDraining(), false);
  assert.equal(coordinator.forceShutdown(), false);
  assert.deepEqual(await coordinator.drain(), { kind: "forced", remaining: 0 });
  assert.equal(coordinator.tryBeginRequest().ok, false);
});
