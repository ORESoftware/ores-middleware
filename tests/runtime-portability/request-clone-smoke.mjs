function fail(message) {
  throw new Error(message);
}

const request = new Request("https://example.test/echo", {
  method: "POST",
  headers: { "content-type": "application/json" },
  body: JSON.stringify({ value: "ok" }),
});
const clone = request.clone();
const first = await request.json();
const second = await clone.json();
if (first.value !== "ok" || second.value !== "ok") fail("Request.clone body semantics drifted");
if (!request.bodyUsed || !clone.bodyUsed) fail("bodyUsed must be true after both bodies are consumed");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "request-clone",
  status: "passed",
}));
