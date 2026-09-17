function fail(message) {
  throw new Error(message);
}

const source = new Headers({ "x-test": "one" });
const request = new Request("https://example.test/", { headers: source });
source.set("x-test", "two");
if (request.headers.get("x-test") !== "one") fail("Request headers unexpectedly track source Headers mutation");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "request-headers-copy",
  status: "passed",
}));
