function fail(message) {
  throw new Error(message);
}

for (const method of ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "HEAD"]) {
  const init = method === "GET" || method === "HEAD" ? { method } : { method, body: "x" };
  const request = new Request("https://example.test/", init);
  if (request.method !== method) fail(`Request method normalization drifted for ${method}: ${request.method}`);
}

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "request-method",
  status: "passed",
}));
