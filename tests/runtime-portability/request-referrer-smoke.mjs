function fail(message) {
  throw new Error(message);
}

const request = new Request("https://example.test/", { referrer: "https://referrer.test/source" });
if (!request.referrer.includes("referrer.test")) fail(`Request referrer drifted: ${request.referrer}`);

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "request-referrer",
  status: "passed",
}));
