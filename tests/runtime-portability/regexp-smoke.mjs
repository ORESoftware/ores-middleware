function fail(message) {
  throw new Error(message);
}

const pattern = /^(?<prefix>ores)-(?<suffix>middleware)$/u;
const match = pattern.exec("ores-middleware");
if (!match?.groups || match.groups.prefix !== "ores" || match.groups.suffix !== "middleware") {
  fail("named regexp group semantics drifted");
}

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "regexp",
  status: "passed",
}));
