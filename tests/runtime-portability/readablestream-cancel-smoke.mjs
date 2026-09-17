function fail(message) {
  throw new Error(message);
}

let cancelled = false;
const stream = new ReadableStream({
  pull(controller) {
    controller.enqueue(new Uint8Array([1]));
  },
  cancel(reason) {
    cancelled = reason === "stop";
  },
});
const reader = stream.getReader();
await reader.read();
await reader.cancel("stop");
if (!cancelled) fail("ReadableStream cancel reason did not propagate");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "readablestream-cancel",
  status: "passed",
}));
