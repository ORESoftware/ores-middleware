function fail(message) {
  throw new Error(message);
}

const value = Object.freeze({ nested: Object.freeze({ id: "stable" }) });
if (!Object.isFrozen(value) || !Object.isFrozen(value.nested)) fail("Object.freeze semantics drifted");
let threw = false;
try {
  value.extra = true;
} catch {
  threw = true;
}
if (!threw && "extra" in value) fail("frozen object was mutated");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "object-freeze",
  status: "passed",
}));
