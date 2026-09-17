import packageMetadata from "../../src/ts/package.json" with { type: "json" };
import { descriptor } from "../../src/ts/dist/index.js";

function fail(message) {
  throw new Error(message);
}

function runtimeName() {
  if (globalThis.Bun) return "bun";
  if (globalThis.Deno) return "deno";
  if (globalThis.process?.versions?.node) return "node";
  return "unknown";
}

const runtime = runtimeName();
if (runtime === "unknown") fail("unsupported JavaScript runtime identity");

const support = packageMetadata.oresRuntimeSupport?.[runtime];
if (typeof support !== "string" || support.length === 0) {
  fail(`missing oresRuntimeSupport metadata for ${runtime}`);
}

const value = descriptor();
if (value.language !== "ts") fail(`unexpected descriptor language ${value.language}`);
if (value.runtime !== "node-deno-bun") fail(`unexpected descriptor runtime ${value.runtime}`);
for (const adapter of ["bun", "deno"]) {
  if (!value.frameworkAdapters.includes(adapter)) {
    fail(`descriptor is missing ${adapter} adapter identity`);
  }
}

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "runtime-metadata",
  status: "passed",
  runtime,
  runtimeSupport: support,
  descriptorRuntime: value.runtime,
}));
