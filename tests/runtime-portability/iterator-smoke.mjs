function fail(message) {
  throw new Error(message);
}

const values = [1, 2, 3];
const mapped = values.map((value) => value * 2);
if (mapped.join(",") !== "2,4,6") fail("Array map semantics drifted");
const seen = [];
for (const value of new Set(values)) seen.push(value);
if (seen.join(",") !== "1,2,3") fail("iterator order drifted");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "iterator",
  status: "passed",
}));
