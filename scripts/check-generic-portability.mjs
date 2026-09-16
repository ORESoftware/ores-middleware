#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const root = process.cwd();

const files = {
  typescript: path.join(root, "src/ts/src/generic.ts"),
  go: path.join(root, "src/golang/generic.go"),
  gleam: path.join(root, "src/gleam/src/ores_middleware/generic.gleam"),
};

const required = {
  typescript: [
    "Provider<",
    "ContextualInput<",
    "Handler<",
    "Middleware<",
    "NamedMiddleware<",
    "composeMiddleware",
    "composeNamedMiddleware",
  ],
  go: [
    "type Provider[",
    "type ContextualInput[",
    "type GenericHandler[",
    "type GenericMiddleware[",
    "type NamedGenericMiddleware[",
    "func ComposeGeneric[",
    "func ComposeNamedGeneric[",
  ],
  gleam: [
    "pub type Provider(",
    "pub type ContextualInput(",
    "pub type Handler(",
    "pub type Middleware(",
    "pub type NamedMiddleware(",
    "pub fn compose(",
    "pub fn compose_named(",
  ],
};

const forbiddenProviderNames = [
  "supabase",
  "clerk",
  "auth0",
  "firebase",
  "okta",
  "cognito",
  "keycloak",
  "workos",
];

function fail(message) {
  console.error(`generic-portability: ${message}`);
  process.exitCode = 1;
}

for (const [language, filename] of Object.entries(files)) {
  if (!fs.existsSync(filename)) {
    fail(`${language}: missing ${path.relative(root, filename)}`);
    continue;
  }

  const source = fs.readFileSync(filename, "utf8");
  for (const symbol of required[language]) {
    if (!source.includes(symbol)) {
      fail(`${language}: missing required generic concept ${JSON.stringify(symbol)}`);
    }
  }

  const lower = source.toLowerCase();
  for (const provider of forbiddenProviderNames) {
    if (lower.includes(provider)) {
      fail(`${language}: generic core mentions concrete provider ${provider}`);
    }
  }
}

if (!process.exitCode) {
  console.log("generic-portability: TypeScript, Go, and Gleam generic cores satisfy the portable contract");
}
