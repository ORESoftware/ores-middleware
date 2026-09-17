function fail(message) {
  throw new Error(message);
}

const value = { z: 1, a: [true, null, "✓"], nested: { n: -0, big: 1e6 } };
const text = JSON.stringify(value);
const parsed = JSON.parse(text);
if (parsed.z !== 1 || parsed.a[2] !== "✓" || parsed.nested.big !== 1e6) fail("JSON round trip drifted");
if (!Object.is(parsed.nested.n, 0)) fail("JSON -0 normalization drifted");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "json",
  status: "passed",
}));
