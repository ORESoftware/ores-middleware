function fail(message) {
  throw new Error(message);
}

const key = {};
const map = new WeakMap();
map.set(key, "value");
if (map.get(key) !== "value") fail("WeakMap get/set mismatch");
if (!map.has(key)) fail("WeakMap has mismatch");
if (!map.delete(key)) fail("WeakMap delete mismatch");
if (map.has(key)) fail("WeakMap retained deleted key");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "weakmap",
  status: "passed",
}));
