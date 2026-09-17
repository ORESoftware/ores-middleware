#!/usr/bin/env node

import { readFile } from "node:fs/promises";

const rootPackagePath = new URL("../package.json", import.meta.url);
const tsPackagePath = new URL("../src/ts/package.json", import.meta.url);
const adapterManifestPath = new URL("../src/ts/adapter.manifest.json", import.meta.url);

const [rootPackage, tsPackage, adapterManifest] = await Promise.all([
  readFile(rootPackagePath, "utf8").then(JSON.parse),
  readFile(tsPackagePath, "utf8").then(JSON.parse),
  readFile(adapterManifestPath, "utf8").then(JSON.parse),
]);

const requiredPortableExports = Object.freeze([
  ".",
  "./generic",
  "./adapters",
  "./bun",
  "./deno",
  "./context",
  "./docs-serving",
]);
const requiredFrameworkExports = Object.freeze(["./koa", "./fastify"]);

const expectedRuntimeSupport = Object.freeze({
  node: ">=22.23.1",
  bun: ">=1.4.2",
  deno: ">=2.9.6",
});

function fail(message) {
  console.error(`ts-package-exports: ${message}`);
  process.exitCode = 1;
}

function normalizedRootTarget(target) {
  return target.replace(/^\.\/src\/ts\//u, "./");
}

function validateTargetShape(subpath, condition, value) {
  if (typeof value !== "string") {
    fail(`${subpath} must expose string ${condition} target`);
    return;
  }
  if (!value.startsWith("./dist/")) {
    fail(`${subpath}.${condition} must stay inside ./dist, got ${value}`);
  }
  const expectedSuffix = condition === "types" ? ".d.ts" : ".js";
  if (!value.endsWith(expectedSuffix)) {
    fail(`${subpath}.${condition} must end with ${expectedSuffix}, got ${value}`);
  }
}

if (rootPackage.name !== tsPackage.name) {
  fail(`package name drift: root=${JSON.stringify(rootPackage.name)} ts=${JSON.stringify(tsPackage.name)}`);
}
if (rootPackage.version !== tsPackage.version) {
  fail(`package version drift: root=${JSON.stringify(rootPackage.version)} ts=${JSON.stringify(tsPackage.version)}`);
}
if (rootPackage.type !== "module" || tsPackage.type !== "module") {
  fail("both root and src/ts package surfaces must remain ESM modules");
}
if (normalizedRootTarget(rootPackage.main ?? "") !== tsPackage.main) {
  fail(`main entrypoint drift: root=${rootPackage.main} ts=${tsPackage.main}`);
}
if (normalizedRootTarget(rootPackage.types ?? "") !== tsPackage.types) {
  fail(`types entrypoint drift: root=${rootPackage.types} ts=${tsPackage.types}`);
}
if (JSON.stringify(rootPackage.engines) !== JSON.stringify(tsPackage.engines)) {
  fail(`engine metadata drift: root=${JSON.stringify(rootPackage.engines)} ts=${JSON.stringify(tsPackage.engines)}`);
}

const rootKeys = Object.keys(rootPackage.exports ?? {}).sort();
const tsKeys = Object.keys(tsPackage.exports ?? {}).sort();
if (JSON.stringify(rootKeys) !== JSON.stringify(tsKeys)) {
  fail(`root/src export keys drift: root=${JSON.stringify(rootKeys)} ts=${JSON.stringify(tsKeys)}`);
}

for (const subpath of [...requiredPortableExports, ...requiredFrameworkExports]) {
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
    validateTargetShape(subpath, condition, tsValue);
  }
}

for (const runtime of ["bun", "deno"]) {
  const target = tsPackage.exports[`./${runtime}`]?.import;
  if (target !== `./dist/${runtime}.js`) {
    fail(`./${runtime} must resolve to ./dist/${runtime}.js, got ${JSON.stringify(target)}`);
  }
}

if (JSON.stringify(tsPackage.oresRuntimeSupport) !== JSON.stringify(expectedRuntimeSupport)) {
  fail(
    `src/ts oresRuntimeSupport must equal tested floors ${JSON.stringify(expectedRuntimeSupport)}`,
  );
}
if (JSON.stringify(rootPackage.oresRuntimeSupport) !== JSON.stringify(expectedRuntimeSupport)) {
  fail(
    `root oresRuntimeSupport must equal tested floors ${JSON.stringify(expectedRuntimeSupport)}`,
  );
}

if (adapterManifest.language !== "ts") {
  fail(`adapter manifest language must be ts, got ${JSON.stringify(adapterManifest.language)}`);
}
if (adapterManifest.runtime !== "node-deno-bun") {
  fail(
    `adapter manifest runtime must be node-deno-bun, got ${JSON.stringify(adapterManifest.runtime)}`,
  );
}
for (const runtimeAdapter of ["bun", "deno"]) {
  if (!Array.isArray(adapterManifest.frameworkAdapters) || !adapterManifest.frameworkAdapters.includes(runtimeAdapter)) {
    fail(`adapter manifest must declare ${runtimeAdapter} framework adapter`);
  }
}

if (!process.exitCode) {
  console.log(
    `ts-package-exports: ${tsKeys.length} export subpaths aligned; Bun/Deno entrypoints, generic/Koa/Fastify roots, engine metadata, Node/Bun/Deno runtime floors, and descriptor claims are consistent`,
  );
}
