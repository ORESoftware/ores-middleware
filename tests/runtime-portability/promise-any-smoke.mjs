function fail(message) {
  throw new Error(message);
}

const result = await Promise.any([
  Promise.reject(new Error("expected")),
  Promise.resolve("accepted"),
]);
if (result !== "accepted") fail(`Promise.any drifted: ${result}`);

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "promise-any",
  status: "passed",
}));
