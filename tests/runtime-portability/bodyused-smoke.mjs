function fail(message) {
  throw new Error(message);
}

const response = new Response("ok");
if (response.bodyUsed) fail("new Response body unexpectedly used");
if ((await response.text()) !== "ok") fail("Response text mismatch");
if (!response.bodyUsed) fail("Response bodyUsed did not flip after consumption");
let rejected = false;
try {
  await response.text();
} catch {
  rejected = true;
}
if (!rejected) fail("second Response body consumption should reject");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "body-used",
  status: "passed",
}));
