function fail(message) {
  throw new Error(message);
}

const response = Response.json({ value: "ok" }, {
  status: 202,
  headers: { "x-ores-test": "yes" },
});
const clone = response.clone();
if (response.status !== 202 || clone.status !== 202) fail("Response clone status drifted");
if (clone.headers.get("x-ores-test") !== "yes") fail("Response clone headers drifted");
if ((await response.json()).value !== "ok") fail("Response body mismatch");
if ((await clone.json()).value !== "ok") fail("Response clone body mismatch");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "response-clone",
  status: "passed",
}));
