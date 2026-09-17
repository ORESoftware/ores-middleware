function fail(message) {
  throw new Error(message);
}

const controller = new AbortController();
const request = new Request("https://example.test/", { signal: controller.signal });
if (request.signal.aborted) fail("new request signal unexpectedly aborted");
controller.abort("shutdown");
if (!request.signal.aborted) fail("request signal did not observe controller abort");
if (request.signal.reason !== "shutdown") fail("request signal reason drifted");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "request-signal",
  status: "passed",
}));
