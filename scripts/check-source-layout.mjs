#!/usr/bin/env node
import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";

const root = new URL("../", import.meta.url);
const required = {
  rust: ["src/rust/Cargo.toml", "src/rust/src/lib.rs"],
  ts: ["src/ts/package.json", "src/ts/src/index.ts"],
  gleam: ["src/gleam/gleam.toml", "src/gleam/src/ores_middleware.gleam"],
  golang: ["src/golang/go.mod", "src/golang/middleware.go"],
  elixir: ["src/elixir/mix.exs", "src/elixir/lib/ores_middleware.ex"],
  erlang: ["src/erlang/rebar.config", "src/erlang/src/ores_middleware.erl"]
};

for (const [language, files] of Object.entries(required)) {
  for (const file of files) await access(new URL(file, root));
  const manifest = JSON.parse(await readFile(new URL(`src/${language}/adapter.manifest.json`, root), "utf8"));
  assert.equal(manifest.language, language);
  const expected = ["descriptor", "defaultConfig", "validateConfig", "createMiddleware", "runWithContext", "currentContext", "capabilities"];
  assert.deepEqual(Object.keys(manifest.operationSymbols).sort(), expected.sort(), `${language} operation surface mismatch`);
}
console.log("source layout and semantic export manifests ok");
