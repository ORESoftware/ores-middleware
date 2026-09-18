#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const [caseId, outputDir] = process.argv.slice(2);
if (!caseId || !outputDir) {
  console.error("usage: mutate-middleware-broad-peer-authority.mjs <case-id> <output-dir>");
  process.exit(64);
}

const sourceDir = path.resolve("contracts/json-schema");
const destination = path.resolve(outputDir);
fs.rmSync(destination, { recursive: true, force: true });
fs.mkdirSync(path.dirname(destination), { recursive: true });
fs.cpSync(sourceDir, destination, { recursive: true });

const files = {
  stack: path.join(destination, "middleware-stack.schema.json"),
  adapter: path.join(destination, "adapter-descriptor.schema.json"),
  runtime: path.join(destination, "runtime-types.schema.json"),
};

const read = (file) => JSON.parse(fs.readFileSync(file, "utf8"));
const write = (file, value) => fs.writeFileSync(file, JSON.stringify(value, null, 2) + "\n");

const mutations = {
  "capability-missing-schema-capture"() {
    const schema = read(files.stack);
    schema.$defs.capability.enum = schema.$defs.capability.enum.filter((value) => value !== "schema-capture");
    write(files.stack, schema);
  },
  "runtime-environment-missing-production"() {
    const schema = read(files.stack);
    schema.$defs.runtimeEnvironment.enum = schema.$defs.runtimeEnvironment.enum.filter((value) => value !== "production");
    write(files.stack, schema);
  },
  "content-representation-missing-html"() {
    const schema = read(files.stack);
    schema.$defs.contentRepresentation.enum = schema.$defs.contentRepresentation.enum.filter((value) => value !== "text/html");
    write(files.stack, schema);
  },
  "rate-limit-capacity-max-drift"() {
    const schema = read(files.stack);
    schema.$defs.rateLimit.properties.capacity.maximum = 2_147_483_646;
    write(files.stack, schema);
  },
  "rate-limit-principal-pattern-drift"() {
    const schema = read(files.stack);
    schema.$defs.rateLimitPrincipal.properties.digest.pattern = "^[0-9a-f]{63}$";
    write(files.stack, schema);
  },
  "adapter-language-missing-erlang"() {
    const schema = read(files.adapter);
    schema.$defs.adapterDescriptor.properties.language.anyOf =
      schema.$defs.adapterDescriptor.properties.language.anyOf.filter((entry) => entry.const !== "erlang");
    write(files.adapter, schema);
  },
  "adapter-operation-symbols-optional"() {
    const schema = read(files.adapter);
    schema.$defs.adapterDescriptor.required =
      schema.$defs.adapterDescriptor.required.filter((value) => value !== "operationSymbols");
    write(files.adapter, schema);
  },
  "shared-auth-fail-open-drift"() {
    const schema = read(files.stack);
    schema.$defs.sharedAuth.properties.failOpen.const = true;
    write(files.stack, schema);
  },
  "string-map-value-type-drift"() {
    const schema = read(files.runtime);
    schema.$defs.StringMap.unevaluatedProperties.type = "integer";
    write(files.runtime, schema);
  },
  "request-context-started-type-drift"() {
    const schema = read(files.runtime);
    schema.$defs.RequestContext.properties.startedAtUnixMs.type = "string";
    write(files.runtime, schema);
  },
  "validation-issue-message-optional"() {
    const schema = read(files.runtime);
    schema.$defs.ValidationIssue.required =
      schema.$defs.ValidationIssue.required.filter((value) => value !== "message");
    write(files.runtime, schema);
  },
  "validation-result-issues-ref-drift"() {
    const schema = read(files.runtime);
    schema.$defs.ValidationResult.properties.issues.items.$ref = "#/$defs/StringMap";
    write(files.runtime, schema);
  },
  "stack-contract-version-drift"() {
    const schema = read(files.stack);
    schema.$defs.middlewareStackConfig.properties.contractVersion.const = "2.0.0";
    write(files.stack, schema);
  },
  "idempotency-method-set-drift"() {
    const schema = read(files.stack);
    schema.$defs.idempotency.properties.requiredMethods.items.anyOf =
      schema.$defs.idempotency.properties.requiredMethods.items.anyOf.filter((entry) => entry.const !== "DELETE");
    write(files.stack, schema);
  },
  "tls-mode-set-drift"() {
    const schema = read(files.stack);
    schema.$defs.tls.properties.mode.anyOf =
      schema.$defs.tls.properties.mode.anyOf.filter((entry) => entry.const !== "trusted-proxy");
    write(files.stack, schema);
  },
};

const mutate = mutations[caseId];
if (!mutate) {
  console.error(`unknown broad peer negative-control case: ${caseId}`);
  process.exit(64);
}

mutate();
console.log(JSON.stringify({
  schema: "ores.middleware.broad-peer-negative-control/v1",
  case: caseId,
  outputDir: destination,
}));
