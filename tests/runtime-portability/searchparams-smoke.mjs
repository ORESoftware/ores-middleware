function fail(message) {
  throw new Error(message);
}

const params = new URLSearchParams();
params.append("x", "1");
params.append("x", "2");
params.set("space", "a b");
if (params.getAll("x").join(",") !== "1,2") fail("URLSearchParams duplicate semantics drifted");
if (params.get("space") !== "a b") fail("URLSearchParams decoding drifted");
params.sort();
if (![...params.keys()].includes("space")) fail("URLSearchParams sort lost values");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "searchparams",
  status: "passed",
}));
