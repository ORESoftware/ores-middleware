#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const root = process.cwd();

const files = {
  rust: [
    path.join(root, "src/rust/src/auth_provider.rs"),
    path.join(root, "src/rust/src/composition.rs"),
  ],
  typescript: [path.join(root, "src/ts/src/generic.ts")],
  go: [path.join(root, "src/golang/generic.go")],
  gleam: [path.join(root, "src/gleam/src/ores_middleware/generic.gleam")],
};

const required = {
  rust: [
    "pub trait StaticAuthVerifier",
    "pub fn auth_provider_fn",
    "pub struct MiddlewareOrderPolicy",
    "pub fn validate_consumer_middleware_order",
  ],
  typescript: [
    "Provider<",
    "FallibleProvider<",
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

const forbiddenTypeScriptRuntimeBindings = [
  "node:",
  "process.",
  "global.process",
  "bun.",
  "deno.",
];

function fail(message) {
  console.error(`generic-portability: ${message}`);
  process.exitCode = 1;
}

for (const [language, filenames] of Object.entries(files)) {
  const missing = filenames.filter((filename) => !fs.existsSync(filename));
  for (const filename of missing) {
    fail(`${language}: missing ${path.relative(root, filename)}`);
  }
  if (missing.length > 0) continue;

  const source = filenames
    .map((filename) => fs.readFileSync(filename, "utf8"))
    .join("\n");
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

  if (language === "typescript") {
    for (const binding of forbiddenTypeScriptRuntimeBindings) {
      if (lower.includes(binding)) {
        fail(`typescript: generic core is coupled to runtime binding ${JSON.stringify(binding)}`);
      }
    }
  }
}

if (!process.exitCode) {
  console.log(
    "generic-portability: Rust, TypeScript, Go, and Gleam generic cores satisfy the portable contract; TypeScript generic core is Node/Bun/Deno neutral",
  );
}
