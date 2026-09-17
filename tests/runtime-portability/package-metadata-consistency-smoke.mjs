import rootPackage from "../../package.json" with { type: "json" };
import tsPackage from "../../src/ts/package.json" with { type: "json" };

function fail(message) {
  throw new Error(message);
}

function normalizeRootTarget(value) {
  return value.replace(/^\.\/src\/ts\//u, "./");
}

const rootExports = Object.keys(rootPackage.exports ?? {}).sort();
const tsExports = Object.keys(tsPackage.exports ?? {}).sort();
if (JSON.stringify(rootExports) !== JSON.stringify(tsExports)) {
  fail(`export map key drift: root=${JSON.stringify(rootExports)} ts=${JSON.stringify(tsExports)}`);
}
for (const key of tsExports) {
  for (const condition of ["types", "import"]) {
    const rootValue = rootPackage.exports[key]?.[condition];
    const tsValue = tsPackage.exports[key]?.[condition];
    if (typeof rootValue !== "string" || typeof tsValue !== "string") {
      fail(`${key}.${condition} must be a string in both package maps`);
    }
    if (normalizeRootTarget(rootValue) !== tsValue) {
      fail(`${key}.${condition} drift: root=${rootValue} ts=${tsValue}`);
    }
  }
}
if (JSON.stringify(rootPackage.oresRuntimeSupport) !== JSON.stringify(tsPackage.oresRuntimeSupport)) {
  fail("oresRuntimeSupport metadata drift");
}

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "package-metadata-consistency",
  status: "passed",
  exports: rootExports.length,
  runtimes: Object.keys(tsPackage.oresRuntimeSupport ?? {}).sort(),
}));
