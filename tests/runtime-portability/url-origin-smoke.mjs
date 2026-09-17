function fail(message) {
  throw new Error(message);
}

const https = new URL("https://example.test:443/path");
const custom = new URL("https://example.test:8443/path");
if (https.origin !== "https://example.test") fail(`default HTTPS origin drifted: ${https.origin}`);
if (custom.origin !== "https://example.test:8443") fail(`custom HTTPS origin drifted: ${custom.origin}`);

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "url-origin",
  status: "passed",
}));
