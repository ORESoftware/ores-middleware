function fail(message) {
  throw new Error(message);
}

const started = Date.now();
await new Promise((resolve) => setTimeout(resolve, 2));
const elapsed = Date.now() - started;
if (elapsed < 0) fail(`Date.now moved backwards during timer smoke: ${elapsed}`);
if (typeof queueMicrotask !== "function") fail("queueMicrotask is unavailable");
let microtaskObserved = false;
queueMicrotask(() => {
  microtaskObserved = true;
});
await Promise.resolve();
if (!microtaskObserved) fail("queueMicrotask did not run before the next promise turn");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "timers",
  status: "passed",
  elapsedMs: elapsed,
}));
