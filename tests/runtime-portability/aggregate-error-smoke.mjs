function fail(message) {
  throw new Error(message);
}

let observed;
try {
  await Promise.any([Promise.reject("a"), Promise.reject("b")]);
} catch (error) {
  observed = error;
}
if (!(observed instanceof AggregateError)) fail("Promise.any rejection must be AggregateError");
if (observed.errors?.join(",") !== "a,b") fail(`AggregateError reasons drifted: ${observed.errors}`);

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "aggregate-error",
  status: "passed",
}));
