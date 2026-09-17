function fail(message) {
  throw new Error(message);
}

const response = new Response("not-json", { headers: { "content-type": "application/json" } });
let threw = false;
try {
  await response.json();
} catch (error) {
  threw = error instanceof Error;
}
if (!threw) fail("invalid JSON response body did not reject");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "response-json-error",
  status: "passed",
}));
