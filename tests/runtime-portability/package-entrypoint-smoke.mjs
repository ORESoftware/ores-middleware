const packageName = "@oresoftware/ores-middleware";

function fail(message) {
  throw new Error(message);
}

const root = await import(packageName);
const generic = await import(`${packageName}/generic`);
const context = await import(`${packageName}/context`);
const adapters = await import(`${packageName}/adapters`);

for (const [surface, module, symbols] of [
  ["root", root, ["defaultConfig", "createMiddleware", "descriptor"]],
  ["generic", generic, ["providerFrom", "composeMiddleware", "composeNamedMiddleware"]],
  ["context", context, ["runWithContext", "currentContext", "bindContext"]],
  ["adapters", adapters, ["fetchHandler", "bunHandler", "denoHandler"]],
]) {
  for (const symbol of symbols) {
    if (typeof module[symbol] === "undefined") {
      fail(`${surface} package entrypoint is missing ${symbol}`);
    }
  }
}

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "package-entrypoints",
  status: "passed",
  packageName,
  surfaces: ["root", "generic", "context", "adapters"],
}));
