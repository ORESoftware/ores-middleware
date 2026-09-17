function fail(message) {
  throw new Error(message);
}

const result = await Promise.all([
  Promise.resolve("a"),
  new Promise((resolve) => setTimeout(() => resolve("b"), 1)),
  "c",
]);
if (result.join(",") !== "a,b,c") fail(`Promise.all order drifted: ${result.join(",")}`);

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "promise-all",
  status: "passed",
}));
