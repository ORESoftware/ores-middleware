function fail(message) {
  throw new Error(message);
}

if (typeof AbortController === "undefined") fail("AbortController is unavailable");

const controller = new AbortController();
let observed = 0;
controller.signal.addEventListener("abort", () => {
  observed += 1;
}, { once: true });
controller.abort("ores-runtime-test");
if (!controller.signal.aborted) fail("AbortSignal did not transition to aborted");
if (controller.signal.reason !== "ores-runtime-test") fail("AbortSignal reason was not preserved");
if (observed !== 1) fail(`abort listener count mismatch: ${observed}`);
controller.abort("second");
if (observed !== 1) fail("AbortSignal fired more than once");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "abort-signal",
  status: "passed",
}));
