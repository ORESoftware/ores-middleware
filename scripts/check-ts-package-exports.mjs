#!/usr/bin/env node

import { readFile } from "node:fs/promises";

const rootPackagePath = new URL("../package.json", import.meta.url);
const tsPackagePath = new URL("../src/ts/package.json", import.meta.url);

const [rootPackage, tsPackage] = await Promise.all([
  readFile(rootPackagePath, "utf8").then(JSON.parse),
  readFile(tsPackagePath, "utf8").then(JSON.parse),
]);

const requiredPortableExports = Object.freeze([
  ".",
  "./generic",
  "./adapters",
  "./context",
  "./docs-serving",
]);

function fail(message) {
  console.error(`ts-package-exports: ${message}`);
  process.exitCode = 1;
}

function normalizedRootTarget(target) {
  return target.replace(/^\.\/src\/ts\//u, "./");
}

const rootKeys = Object.keys(rootPackage.exports ?? {}).sort();
const tsKeys = Object.keys(tsPackage.exports ?? {}).sort();
if (JSON.stringify(rootKeys) !== JSON.stringify(tsKeys)) {
  fail(`root/src export keys drift: root=${JSON.stringify(rootKeys)} ts=${JSON.stringify(tsKeys)}`);
}

for (const subpath of requiredPortableExports) {
  if (!(subpath in (rootPackage.exports ?? {}))) fail(`root package missing ${subpath}`);
  if (!(subpath in (tsPackage.exports ?? {}))) fail(`src/ts package missing ${subpath}`);
}

for (const subpath of tsKeys) {
  const rootTarget = rootPackage.exports[subpath];
  const tsTarget = tsPackage.exports[subpath];
  for (const condition of ["types", "import"]) {
    const rootValue = rootTarget?.[condition];
    const tsValue = tsTarget?.[condition];
    if (typeof rootValue !== "string" || typeof tsValue !== "string") {
      fail(`${subpath} must expose string ${condition} targets in both package maps`);
      continue;
    }
    if (normalizedRootTarget(rootValue) !== tsValue) {
      fail(`${subpath}.${condition} drift: root=${rootValue} ts=${tsValue}`);
    }
  }
}

const runtime = tsPackage.oresRuntimeSupport;
if (!runtime || typeof runtime !== "object") {
  fail("src/ts package must declare oresRuntimeSupport");
} else {
  for (const name of ["node", "bun", "deno"]) {
    if (typeof runtime[name] !== "string" || runtime[name].length === 0) {
      fail(`src/ts package must declare oresRuntimeSupport.${name}`);
    }
  }
}

if (JSON.stringify(rootPackage.oresRuntimeSupport) !== JSON.stringify(tsPackage.oresRuntimeSupport)) {
  fail("root/src oresRuntimeSupport metadata must match exactly");
}

if (!process.exitCode) {
  console.log(
    `ts-package-exports: ${tsKeys.length} export subpaths aligned; Node/Bun/Deno runtime metadata present`,
  );
}
