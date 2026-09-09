/**
 * Routing-neutral docs-serving contract.
 *
 * This file and docs-serving.schema.json are independent, human-authored peer
 * authorities. Neither file may be generated from or overwrite the other.
 */
import "@typespec/json-schema";
using TypeSpec.JsonSchema;
namespace Ores.Middleware.Docs;

enum DocsRepresentation {
  html: "html",
  catalog: "catalog",
  openapi: "openapi",
  openrpc: "openrpc",
  connect: "connect",
  hyperSchema: "hyper-schema",
}

enum DocsAction {
  pass: "pass",
  serve: "serve",
  methodNotAllowed: "method-not-allowed",
  notAcceptable: "not-acceptable",
  stoppedForEvaluation: "stopped-for-evaluation",
}

// Keep explicit additional-property closure for consumers that inspect it.
// The pinned emitter also records unevaluated-property closure.
@extension("additionalProperties", false)
model DocsRequest {
  method: string;
  path: string;
  accept?: string;
  format?: string;
  @pattern("^[0-9a-f]{64}$")
  runtimeContractDigest?: string;
  @pattern("^[0-9a-f]{64}$")
  docsContractDigest?: string;
}

// A named wire contract, not an emitter-generated RecordString declaration.
@extension("additionalProperties", #{ type: "string" })
model DocsHeaders is Record<string>;

@extension("additionalProperties", false)
model DocsDecision {
  action: DocsAction;
  status?: uint16;
  representation?: DocsRepresentation;
  headOnly: boolean;
  headers: DocsHeaders;
}
