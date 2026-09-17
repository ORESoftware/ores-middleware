function fail(message) {
  throw new Error(message);
}

const response = Response.json({ ok: true });
if (!String(response.headers.get("content-type")).toLowerCase().includes("application/json")) {
  fail(`Response.json content-type drifted: ${response.headers.get("content-type")}`);
}
if ((await response.json()).ok !== true) fail("Response.json body drifted");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "response-json",
  status: "passed",
}));
