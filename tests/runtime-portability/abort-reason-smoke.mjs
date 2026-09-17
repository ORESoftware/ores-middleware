function fail(message) {
  throw new Error(message);
}

const reason = Object.freeze({ code: "shutdown" });
const controller = new AbortController();
controller.abort(reason);
if (controller.signal.reason !== reason) fail("AbortSignal did not preserve object reason identity");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "abort-reason",
  status: "passed",
}));
