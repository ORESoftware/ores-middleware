function fail(message) {
  throw new Error(message);
}

let threw = false;
try {
  new Headers({ "bad header": "x" });
} catch {
  threw = true;
}
if (!threw) fail("invalid HTTP header name was accepted");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "headers-invalid-name",
  status: "passed",
}));
