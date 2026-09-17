function fail(message) {
  throw new Error(message);
}

if (typeof AbortSignal?.any !== "function") {
  fail("AbortSignal.any is unavailable");
}
const first = new AbortController();
const second = new AbortController();
const signal = AbortSignal.any([first.signal, second.signal]);
second.abort("second");
if (!signal.aborted) fail("AbortSignal.any did not abort");
if (signal.reason !== "second") fail("AbortSignal.any did not preserve first observed reason");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "abort-any",
  status: "passed",
}));
