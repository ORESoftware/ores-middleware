function fail(message) {
  throw new Error(message);
}

const map = new Map([["a", 1], ["b", 2]]);
const set = new Set(["a", "b", "a"]);
if ([...map.keys()].join(",") !== "a,b") fail("Map insertion order drifted");
if ([...set].join(",") !== "a,b") fail("Set uniqueness/insertion order drifted");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "map-set",
  status: "passed",
}));
