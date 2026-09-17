function fail(message) {
  throw new Error(message);
}

if (typeof structuredClone !== "function") fail("structuredClone is unavailable");
const source = { nested: { value: "ok" }, list: [1, 2, 3] };
const cloned = structuredClone(source);
source.nested.value = "mutated";
source.list.push(4);
if (cloned.nested.value !== "ok") fail("structuredClone shared nested state");
if (cloned.list.length !== 3) fail("structuredClone shared array state");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "structured-clone",
  status: "passed",
}));
