import * as root from "../../target/ts/dist/index.js";
import * as generic from "../../target/ts/dist/generic.js";
import * as context from "../../target/ts/dist/context.js";
import * as adapters from "../../target/ts/dist/adapters.js";

function fail(message) {
  throw new Error(message);
}

for (const [surface, module, symbols] of [
  ["root", root, ["defaultConfig", "createMiddleware", "descriptor"]],
  ["generic", generic, ["providerFrom", "composeMiddleware", "composeNamedMiddleware"]],
  ["context", context, ["runWithContext", "currentContext", "bindContext"]],
  ["adapters", adapters, ["fetchHandler", "bunHandler", "denoHandler"]],
]) {
  for (const symbol of symbols) {
    if (typeof module[symbol] === "undefined") fail(`${surface} packaged entrypoint missing ${symbol}`);
  }
}

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "package-entrypoints-relative",
  status: "passed",
  surfaces: ["root", "generic", "context", "adapters"],
}));
