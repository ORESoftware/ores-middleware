function fail(message) {
  throw new Error(message);
}

let threw = false;
try {
  new TextDecoder("utf-8", { fatal: true }).decode(Uint8Array.of(0xff));
} catch {
  threw = true;
}
if (!threw) fail("fatal TextDecoder accepted invalid UTF-8");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "encoding-errors",
  status: "passed",
}));
