function fail(message) {
  throw new Error(message);
}

const result = await Promise.race([
  Promise.resolve("microtask"),
  new Promise((resolve) => setTimeout(() => resolve("timer"), 1)),
]);
if (result !== "microtask") fail(`Promise.race ordering drifted: ${result}`);

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "promise-race",
  status: "passed",
}));
