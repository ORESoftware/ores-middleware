#!/usr/bin/env node
import assert from "node:assert/strict";
import { readdir, readFile } from "node:fs/promises";
import { basename, resolve } from "node:path";
import Ajv2020 from "ajv/dist/2020.js";

const root = resolve(new URL("..", import.meta.url).pathname);
const schema = JSON.parse(await readFile(resolve(root, "contracts/json-schema/adapter-descriptor.schema.json"), "utf8"));
const validate = new Ajv2020({ allErrors: true, strict: true }).compile(schema);
const requireAll = process.argv.includes("--require-all");
const explicit = process.argv.filter((arg) => !arg.startsWith("--") && arg !== process.argv[0] && arg !== process.argv[1]);
let files = explicit;
if (files.length === 0) {
  const directory = resolve(root, "target/descriptors");
  files = (await readdir(directory, { withFileTypes: true })).filter((item) => item.isFile() && item.name.endsWith(".json")).map((item) => resolve(directory, item.name));
}

const seen = new Set();
for (const file of files) {
  const descriptor = JSON.parse(await readFile(file, "utf8"));
  assert(validate(descriptor), `${file}: ${JSON.stringify(validate.errors, null, 2)}`);
  assert(!seen.has(descriptor.language), `duplicate descriptor for ${descriptor.language}`);
  seen.add(descriptor.language);
  console.log(`descriptor ok: ${descriptor.language} (${basename(file)})`);
}

if (requireAll) {
  const expected = ["rust", "ts", "gleam", "golang", "elixir", "erlang"];
  assert.deepEqual([...seen].sort(), expected.sort(), `expected descriptors for ${expected.join(", ")}`);
}
