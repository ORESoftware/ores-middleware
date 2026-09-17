function fail(message) {
  throw new Error(message);
}

const request = new Request("https://example.test/", { credentials: "same-origin" });
if (request.credentials !== "same-origin") fail(`Request credentials drifted: ${request.credentials}`);

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "request-credentials",
  status: "passed",
}));
