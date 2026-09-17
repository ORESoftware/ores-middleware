function fail(message) {
  throw new Error(message);
}

const response = Response.redirect("https://example.test/next", 307);
if (response.status !== 307) fail(`redirect status drifted: ${response.status}`);
if (response.headers.get("location") !== "https://example.test/next") fail(`redirect location drifted: ${response.headers.get("location")}`);

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "response-redirect",
  status: "passed",
}));
