export type DocsRepresentation =
  | "html"
  | "catalog"
  | "openapi"
  | "openrpc"
  | "connect"
  | "hyper-schema";

export type DocsAction =
  | "pass"
  | "serve"
  | "method-not-allowed"
  | "not-acceptable"
  | "stopped-for-evaluation";

export interface DocsRequest {
  method: string;
  path: string;
  accept?: string;
  format?: string;
  runtimeContractDigest?: string;
  docsContractDigest?: string;
}

export interface DocsDecision {
  action: DocsAction;
  status?: number;
  representation?: DocsRepresentation;
  headOnly: boolean;
  headers: Readonly<Record<string, string>>;
}

export declare const DOCS_FORMAT_HEADER: "X-Ores-Docs-Format";
export declare const CONTRACT_DIGEST_HEADER: "X-Ores-Contract-SHA256";
export declare function decideDocs(request: Readonly<DocsRequest>): DocsDecision;
