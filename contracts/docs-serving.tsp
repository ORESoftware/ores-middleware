/**
 * Routing-neutral docs-serving contract.
 *
 * This file and docs-serving.schema.json are independent, human-authored peer
 * authorities. Neither file may be generated from or overwrite the other.
 */
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

model DocsDecision {
  action: DocsAction;
  status?: uint16;
  representation?: DocsRepresentation;
  headOnly: boolean;
  headers: Record<string>;
}
