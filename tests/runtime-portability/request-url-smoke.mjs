function fail(message) {
  throw new Error(message);
}

const request = new Request("https://example.test/a%20b?x=1#fragment");
if (request.url !== "https://example.test/a%20b?x=1#fragment") fail(`Request URL drifted: ${request.url}`);
const parsed = new URL(request.url);
if (parsed.pathname !== "/a%20b" || parsed.searchParams.get("x") !== "1") fail("Request URL parse drifted");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "request-url",
  status: "passed",
}));
