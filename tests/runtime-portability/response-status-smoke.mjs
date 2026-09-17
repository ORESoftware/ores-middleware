function fail(message) {
  throw new Error(message);
}

for (const status of [200, 201, 202, 204, 400, 401, 403, 404, 409, 413, 429, 500, 503]) {
  const response = new Response(status === 204 ? null : "x", { status });
  if (response.status !== status) fail(`Response status drifted for ${status}: ${response.status}`);
  if (response.ok !== (status >= 200 && status < 300)) fail(`Response ok drifted for ${status}`);
}

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "response-status",
  status: "passed",
}));
