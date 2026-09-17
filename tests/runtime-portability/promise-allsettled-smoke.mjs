function fail(message) {
  throw new Error(message);
}

const sentinel = new Error("expected");
const result = await Promise.allSettled([
  Promise.resolve("ok"),
  Promise.reject(sentinel),
]);
if (result[0]?.status !== "fulfilled" || result[0].value !== "ok") fail("Promise.allSettled fulfillment drifted");
if (result[1]?.status !== "rejected" || result[1].reason !== sentinel) fail("Promise.allSettled rejection identity drifted");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "promise-allsettled",
  status: "passed",
}));
