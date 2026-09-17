function fail(message) {
  throw new Error(message);
}

const headers = new Headers([["b", "2"], ["a", "1"]]);
const entries = [...headers.entries()];
if (!entries.some(([key, value]) => key === "a" && value === "1")) fail("Headers iteration lost a");
if (!entries.some(([key, value]) => key === "b" && value === "2")) fail("Headers iteration lost b");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "headers-iteration",
  status: "passed",
  entries,
}));
