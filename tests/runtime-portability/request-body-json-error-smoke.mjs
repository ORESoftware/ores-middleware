function fail(message) {
  throw new Error(message);
}

const request = new Request("https://example.test/", {
  method: "POST",
  headers: { "content-type": "application/json" },
  body: "not-json",
});
let threw = false;
try {
  await request.json();
} catch (error) {
  threw = error instanceof Error;
}
if (!threw) fail("invalid JSON request body did not reject");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "request-json-error",
  status: "passed",
}));
