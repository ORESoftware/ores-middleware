import packageMetadata from "../../src/ts/package.json" with { type: "json" };

function fail(message) {
  throw new Error(message);
}

function tuple(version) {
  return String(version)
    .replace(/^v/u, "")
    .split(/[.-]/u)
    .slice(0, 3)
    .map((value) => Number.parseInt(value, 10));
}

function atLeast(actual, minimum) {
  const left = tuple(actual);
  const right = tuple(minimum);
  for (let index = 0; index < 3; index += 1) {
    const a = left[index] ?? 0;
    const b = right[index] ?? 0;
    if (a > b) return true;
    if (a < b) return false;
  }
  return true;
}

let runtime;
let actual;
if (globalThis.Bun) {
  runtime = "bun";
  actual = globalThis.Bun.version;
} else if (globalThis.Deno) {
  runtime = "deno";
  actual = globalThis.Deno.version.deno;
} else if (globalThis.process?.versions?.node) {
  runtime = "node";
  actual = globalThis.process.versions.node;
} else {
  fail("unknown JavaScript runtime");
}

const declared = packageMetadata.oresRuntimeSupport?.[runtime];
if (typeof declared !== "string" || !declared.startsWith(">=")) {
  fail(`missing >= runtime floor for ${runtime}`);
}
const minimum = declared.slice(2);
if (!atLeast(actual, minimum)) {
  fail(`${runtime} ${actual} is below declared runtime floor ${minimum}`);
}

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "runtime-version-floor",
  status: "passed",
  runtime,
  actual,
  minimum,
}));
