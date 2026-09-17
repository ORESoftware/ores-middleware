function fail(message) {
  throw new Error(message);
}

const target = {};
if (!Reflect.set(target, "value", 42)) fail("Reflect.set failed");
if (Reflect.get(target, "value") !== 42) fail("Reflect.get mismatch");
if (!Reflect.has(target, "value")) fail("Reflect.has mismatch");
if (!Reflect.deleteProperty(target, "value")) fail("Reflect.deleteProperty failed");
if (Reflect.has(target, "value")) fail("Reflect.deleteProperty did not remove value");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "reflect",
  status: "passed",
}));
