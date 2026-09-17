function fail(message) {
  throw new Error(message);
}

const headers = new Headers();
headers.append("X-ORES-Test", "one");
headers.append("x-ores-test", "two");
if (headers.get("X-ORES-TEST") === null) fail("header lookup must be case-insensitive");
if (headers.get("x-ores-test") !== headers.get("X-ORES-Test")) fail("header casing changed lookup value");
headers.set("X-Request-ID", "abc");
if (headers.get("x-request-id") !== "abc") fail("canonical request-id lookup failed");
headers.delete("X-REQUEST-ID");
if (headers.has("x-request-id")) fail("case-insensitive header delete failed");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "headers-semantics",
  status: "passed",
}));
