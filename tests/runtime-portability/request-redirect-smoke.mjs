function fail(message) {
  throw new Error(message);
}

const request = new Request("https://example.test/", { redirect: "manual" });
if (request.redirect !== "manual") fail(`Request redirect mode drifted: ${request.redirect}`);

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "request-redirect",
  status: "passed",
}));
