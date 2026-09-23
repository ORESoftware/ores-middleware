import type { AuthDecision, RequestContext } from "./index.js";

/**
 * Provider-neutral Fetch capability injected by the host.
 * Authored edge middleware should use this function rather than importing a
 * provider-specific HTTP client or opening sockets directly.
 */
export type EdgeFetchProvider = (
  input: RequestInfo | URL,
  init?: RequestInit
) => Promise<Response>;

/** Auth is bound to the current request by the host so the restricted callback never receives the raw Request. */
export interface EdgeAuthProvider {
  verify(): Promise<AuthDecision>;
}

export interface EdgeRateLimitProvider {
  allow(key: string, capacity: number, refillPerSecond: number): Promise<boolean>;
}

export interface EdgeCacheProvider {
  get(key: string): Promise<Uint8Array | undefined>;
  set(key: string, value: Uint8Array, ttlMs?: number): Promise<void>;
  delete(key: string): Promise<void>;
}

export interface EdgeTelemetryProvider {
  event(name: string, fields?: Readonly<Record<string, string | number | boolean>>): Promise<void> | void;
}

/**
 * Host-owned provider implementations. These are flattened into callback args
 * so authored middleware can destructure everything it is allowed to use and
 * import nothing provider-specific.
 */
export interface EdgeMinimalDependencies {
  fetch: EdgeFetchProvider;
  auth: EdgeAuthProvider;
  rateLimit: EdgeRateLimitProvider;
  cache: EdgeCacheProvider;
  telemetry: EdgeTelemetryProvider;
}

const ingressOwnedRequestHeaders = new Set([
  "host",
  "content-length",
  "transfer-encoding",
  "trailer",
  "expect",
  "connection",
  "keep-alive",
  "proxy-connection",
  "upgrade",
  "te"
]);

function canonicalHeaderName(name: string): string {
  const canonical = name.toLowerCase();
  if (!/^[a-z0-9!#$%&'*+.^_`|~-]+$/.test(canonical)) {
    throw new EdgeMinimalContractError("edge_header_name_invalid", "edge middleware header name is invalid");
  }
  return canonical;
}

function assertMutableHeader(name: string): string {
  const canonical = canonicalHeaderName(name);
  if (ingressOwnedRequestHeaders.has(canonical)) {
    throw new EdgeMinimalContractError(
      "edge_ingress_header_owned_by_host",
      `edge_minimal middleware may not mutate ingress-owned header ${canonical}`
    );
  }
  return canonical;
}

/**
 * Restricted request view for default P2 middleware.
 *
 * Method, URL and path are read-only. The raw Request and body are intentionally
 * absent. Authored middleware may inspect headers and mutate only semantic
 * request headers; transport/authority/framing headers remain host-owned.
 */
export class EdgeMinimalRequest {
  readonly #method: string;
  readonly #url: string;
  readonly #headers: Headers;

  constructor(request: Request) {
    this.#method = request.method;
    this.#url = request.url;
    this.#headers = new Headers(request.headers);
  }

  get method(): string {
    return this.#method;
  }

  get url(): string {
    return this.#url;
  }

  get path(): string {
    return new URL(this.#url).pathname;
  }

  header(name: string): string | null {
    return this.#headers.get(name);
  }

  hasHeader(name: string): boolean {
    return this.#headers.has(name);
  }

  setHeader(name: string, value: string): void {
    this.#headers.set(assertMutableHeader(name), value);
  }

  appendHeader(name: string, value: string): void {
    this.#headers.append(assertMutableHeader(name), value);
  }

  removeHeader(name: string): void {
    this.#headers.delete(assertMutableHeader(name));
  }

  /** Defensive copy used by host adapters when handing the admitted request to P3. */
  headersSnapshot(): Headers {
    return new Headers(this.#headers);
  }
}

export interface EdgeMinimalCallbackArgs extends EdgeMinimalDependencies {
  request: EdgeMinimalRequest;
  context: Readonly<RequestContext>;
}

export type EdgeMinimalDecision =
  | { kind: "continue" }
  | { kind: "respond"; response: Response };

/**
 * This is intentionally a plain function type. A generated/user middleware
 * module can simply `export default async ({ request, fetch, ... }) => ...`
 * without importing ores-middleware or a provider SDK.
 */
export type EdgeMinimalCallback = (
  args: EdgeMinimalCallbackArgs
) => EdgeMinimalDecision | Promise<EdgeMinimalDecision>;

export type EdgeMinimalHostResult =
  | { kind: "continue"; request: Request }
  | { kind: "respond"; response: Response };

export class EdgeMinimalContractError extends Error {
  constructor(
    public readonly code: string,
    message: string
  ) {
    super(message);
    this.name = "EdgeMinimalContractError";
  }
}

function readonlyContext(context: RequestContext): Readonly<RequestContext> {
  return Object.freeze({
    ...context,
    baggage: Object.freeze({ ...context.baggage })
  });
}

function validateShortCircuitResponse(response: Response): void {
  if (response.status < 200 || response.status > 599) {
    throw new EdgeMinimalContractError(
      "edge_response_status_invalid",
      "edge_minimal short-circuit response must use a final HTTP status"
    );
  }
  for (const name of response.headers.keys()) {
    const canonical = canonicalHeaderName(name);
    if (ingressOwnedRequestHeaders.has(canonical)) {
      throw new EdgeMinimalContractError(
        "edge_response_framing_owned_by_host",
        `edge_minimal response may not author transport/framing header ${canonical}`
      );
    }
  }
}

/**
 * Host-side adapter entrypoint for a restricted P2 callback.
 *
 * The callback receives no raw Request, body, filesystem, process, socket, DB,
 * provider SDK or downstream `next`. It receives only the approved dependency
 * set and a header-mutable/read-only-structure request view.
 */
export async function invokeEdgeMinimal(
  callback: EdgeMinimalCallback,
  request: Request,
  context: RequestContext,
  dependencies: EdgeMinimalDependencies
): Promise<EdgeMinimalHostResult> {
  const requestView = new EdgeMinimalRequest(request);
  const decision = await callback({
    request: requestView,
    context: readonlyContext(context),
    fetch: dependencies.fetch,
    auth: dependencies.auth,
    rateLimit: dependencies.rateLimit,
    cache: dependencies.cache,
    telemetry: dependencies.telemetry
  });

  if (decision.kind === "respond") {
    validateShortCircuitResponse(decision.response);
    return decision;
  }

  return {
    kind: "continue",
    request: new Request(request, { headers: requestView.headersSnapshot() })
  };
}
