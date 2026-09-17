function fail(message) {
  throw new Error(message);
}

if (typeof AbortSignal?.timeout !== "function") {
  fail("AbortSignal.timeout is unavailable");
}
const signal = AbortSignal.timeout(1);
if (signal.aborted) fail("AbortSignal.timeout aborted synchronously");
await new Promise((resolve) => signal.addEventListener("abort", resolve, { once: true }));
if (!signal.aborted) fail("AbortSignal.timeout never aborted");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "abort-timeout",
  status: "passed",
}));
