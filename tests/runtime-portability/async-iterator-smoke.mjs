function fail(message) {
  throw new Error(message);
}

async function* values() {
  yield 1;
  await Promise.resolve();
  yield 2;
}
const observed = [];
for await (const value of values()) observed.push(value);
if (observed.join(",") !== "1,2") fail(`async iterator semantics drifted: ${observed.join(",")}`);

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "async-iterator",
  status: "passed",
}));
