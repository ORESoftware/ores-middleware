function fail(message) {
  throw new Error(message);
}

const date = new Date("2026-09-17T12:34:56.789Z");
if (date.toISOString() !== "2026-09-17T12:34:56.789Z") fail(`Date ISO semantics drifted: ${date.toISOString()}`);
if (Date.parse(date.toISOString()) !== date.getTime()) fail("Date parse/getTime round trip drifted");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "date",
  status: "passed",
}));
