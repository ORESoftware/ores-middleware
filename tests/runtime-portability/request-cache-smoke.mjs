function fail(message) {
  throw new Error(message);
}

const request = new Request("https://example.test/", { cache: "no-store" });
if (request.cache !== "no-store") fail(`Request cache drifted: ${request.cache}`);

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "request-cache",
  status: "passed",
}));
