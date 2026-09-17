function fail(message) {
  throw new Error(message);
}

const events = [];
queueMicrotask(() => events.push("microtask"));
Promise.resolve().then(() => events.push("promise"));
setTimeout(() => events.push("timer"), 0);
await Promise.resolve();
await Promise.resolve();
if (!events.includes("microtask") || !events.includes("promise")) fail(`microtasks not observed: ${events.join(",")}`);
await new Promise((resolve) => setTimeout(resolve, 1));
if (events.at(-1) !== "timer") fail(`timer ordering drifted: ${events.join(",")}`);

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "promise-order",
  status: "passed",
  events,
}));
