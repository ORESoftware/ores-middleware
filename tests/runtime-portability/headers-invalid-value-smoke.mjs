function fail(message) {
  throw new Error(message);
}

let threw = false;
try {
  new Headers({ "x-test": "ok\nnot-ok" });
} catch {
  threw = true;
}
if (!threw) fail("invalid HTTP header value with newline was accepted");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "headers-invalid-value",
  status: "passed",
}));
