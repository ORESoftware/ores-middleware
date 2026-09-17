function fail(message) {
  throw new Error(message);
}

const stream = new ReadableStream({
  start(controller) {
    controller.enqueue(new TextEncoder().encode("ok"));
    controller.close();
  },
});
let request;
try {
  request = new Request("https://example.test/", { method: "POST", body: stream, duplex: "half" });
} catch (error) {
  console.log(JSON.stringify({
    schema: "ores.middleware.js-runtime-portability-smoke/v1",
    suite: "request-duplex",
    status: "skipped",
    reason: String(error?.message ?? error),
  }));
}
if (request) {
  if ((await request.text()) !== "ok") fail("streaming Request body drifted");
  console.log(JSON.stringify({
    schema: "ores.middleware.js-runtime-portability-smoke/v1",
    suite: "request-duplex",
    status: "passed",
  }));
}
