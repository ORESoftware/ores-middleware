function fail(message) {
  throw new Error(message);
}

const events = [];
const sentinel = new Error("sentinel");
let observed;
try {
  try {
    events.push("try");
    await Promise.resolve();
    throw sentinel;
  } finally {
    events.push("finally");
    await Promise.resolve();
  }
} catch (error) {
  observed = error;
  events.push("catch");
}
if (observed !== sentinel) fail("async finally replaced thrown error identity");
if (events.join(",") !== "try,finally,catch") fail(`async finally order drifted: ${events.join(",")}`);

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "async-finally",
  status: "passed",
}));
