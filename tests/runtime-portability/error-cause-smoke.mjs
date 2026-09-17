function fail(message) {
  throw new Error(message);
}

const cause = new Error("root-cause");
const outer = new Error("outer", { cause });
if (outer.cause !== cause) fail("Error cause identity was not preserved");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "error-cause",
  status: "passed",
}));
