function fail(message) {
  throw new Error(message);
}

const source = new Headers({ "x-test": "one" });
const response = new Response("ok", { headers: source });
source.set("x-test", "two");
if (response.headers.get("x-test") !== "one") fail("Response headers unexpectedly track source Headers mutation");
response.headers.set("x-second", "yes");
if (response.headers.get("x-second") !== "yes") fail("Response headers mutation failed");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "response-headers-copy",
  status: "passed",
}));
