function fail(message) {
  throw new Error(message);
}

const headers = new Headers({ "content-length": "123" });
if (headers.get("Content-Length") !== "123") fail("content-length header lookup drifted");
headers.set("CONTENT-LENGTH", "456");
if (headers.get("content-length") !== "456") fail("content-length header set drifted");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "content-length-header",
  status: "passed",
}));
