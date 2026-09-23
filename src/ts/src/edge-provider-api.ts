import type { AuthDecision, RequestContext } from "./index.js";

export type EdgeLogScalar = string | number | boolean | null;
export type EdgeLogFields = Readonly<Record<string, EdgeLogScalar>>;

export interface EdgeLogger {
  debug(message: string, fields?: EdgeLogFields): void;
  info(message: string, fields?: EdgeLogFields): void;
  warn(message: string, fields?: EdgeLogFields): void;
  error(message: string, fields?: EdgeLogFields): void;
}

export interface EdgeFetchRequest {
  method: string;
  url: string;
  headers?: Readonly<Record<string, string>>;
  body?: Uint8Array;
}

export interface EdgeFetchResponse {
  status: number;
  headers: Readonly<Record<string, string>>;
  body: Uint8Array;
}

export interface EdgeFetchProvider {
  fetch(request: EdgeFetchRequest): Promise<EdgeFetchResponse>;
}

export interface EdgeAuthProvider {
  verify(request: EdgeMinimalRequest, context: Readonly<RequestContext>): Promise<AuthDecision>;
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
  started(context: Readonly<RequestContext>, request: EdgeMinimalRequest): Promise<void> | void;
  finished(
    context: Readonly<RequestContext>,
    request: EdgeMinimalRequest,
    terminal: EdgeTerminalMetadata
  ): Promise<void> | void;
}

export interface EdgeTerminalMetadata {
  outcome: "completed" | "disconnected" | "timed_out" | "child_failed";
  status?: number;
  responseBytes: number;
  elapsedMs: number;
}

/**
 * Complete provider-neutral dependency bag approved for portable P2 middleware.
 * Provider SDKs stay in the host adapter; callbacks receive only these capabilities.
 */
export interface EdgeApprovedDependencies {
  fetch: EdgeFetchProvider;
  auth: EdgeAuthProvider;
  rateLimiter: EdgeRateLimitProvider;
  cache: EdgeCacheProvider;
  telemetry: EdgeTelemetryProvider;
  log: EdgeLogger;
  now(): number;
  randomId(): string;
}

export interface EdgeTrustedRequestFacts {
  remoteIp?: string;
  contentLength?: number;
  transportSecure: boolean;
}

/**
 * Request view for `edge_minimal`. Method/path/body/transport facts are immutable;
 * only request headers may be changed by authored middleware.
 */
export class EdgeMinimalRequest {
  readonly method: string;
  readonly url: string;
  readonly path: string;
  readonly remoteIp?: string;
  readonly contentLength?: number;
  readonly transportSecure: boolean;
  readonly #headers: Map<string, string>;

  constructor(request: Request, facts: EdgeTrustedRequestFacts) {
    const url = new URL(request.url);
    this.method = request.method;
    this.url = request.url;
    this.path = `${url.pathname}${url.search}`;
    this.remoteIp = facts.remoteIp;
    this.contentLength = facts.contentLength;
    this.transportSecure = facts.transportSecure;
    this.#headers = new Map(
      [...request.headers.entries()].map(([name, value]) => [name.toLowerCase(), value])
    );
  }

  header(name: string): string | undefined {
    return this.#headers.get(name.toLowerCase());
  }

  headers(): Readonly<Record<string, string>> {
    return Object.freeze(Object.fromEntries(this.#headers));
  }

  setHeader(name: string, value: string): void {
    this.#headers.set(normalizeHeaderName(name), validateHeaderValue(value));
  }

  removeHeader(name: string): boolean {
    return this.#headers.delete(name.toLowerCase());
  }

  /** Host-adapter projection used for handoff after admission. */
  toRequestHeaders(): Headers {
    return new Headers([...this.#headers.entries()]);
  }
}

export type EdgeMinimalDecision =
  | { readonly kind: "continue"; readonly request: EdgeMinimalRequest }
  | { readonly kind: "respond"; readonly response: Response };

/**
 * Everything a portable P2 callback is allowed to use is injected directly in
 * its parameter object. There is no filesystem, DB, raw socket, process, or
 * provider-SDK escape hatch on this type.
 */
export interface EdgeMinimalCallbackArgs extends EdgeApprovedDependencies {
  request: EdgeMinimalRequest;
  context: Readonly<RequestContext>;
  continue(): EdgeMinimalDecision;
  respond(response: Response): EdgeMinimalDecision;
  setRequestHeader(name: string, value: string): void;
  removeRequestHeader(name: string): boolean;
}

export type EdgeMinimalMiddleware = (
  args: EdgeMinimalCallbackArgs
) => Promise<EdgeMinimalDecision> | EdgeMinimalDecision;

export function defineEdgeMinimalMiddleware(callback: EdgeMinimalMiddleware): EdgeMinimalMiddleware {
  return callback;
}

export type EdgeNext = (request: Request) => Promise<Response>;

/** Response-aware portable profile. It remains provider-neutral but stays in the data path. */
export interface EdgeFetchCallbackArgs extends EdgeApprovedDependencies {
  request: Request;
  context: Readonly<RequestContext>;
  next: EdgeNext;
}

export type EdgeFetchMiddleware = (
  args: EdgeFetchCallbackArgs
) => Promise<Response> | Response;

export function defineEdgeFetchMiddleware(callback: EdgeFetchMiddleware): EdgeFetchMiddleware {
  return callback;
}

/** Host utility: flatten approved capabilities directly onto callback params. */
export function createEdgeMinimalCallbackArgs(
  request: EdgeMinimalRequest,
  context: Readonly<RequestContext>,
  dependencies: EdgeApprovedDependencies
): EdgeMinimalCallbackArgs {
  return {
    ...dependencies,
    request,
    context,
    continue: () => ({ kind: "continue", request }),
    respond: (response) => ({ kind: "respond", response }),
    setRequestHeader: (name, value) => request.setHeader(name, value),
    removeRequestHeader: (name) => request.removeHeader(name)
  };
}

function normalizeHeaderName(name: string): string {
  const normalized = name.toLowerCase();
  if (!/^[a-z0-9!#$%&'*+.^_`|~-]+$/.test(normalized)) {
    throw new TypeError("invalid middleware request header name");
  }
  return normalized;
}

function validateHeaderValue(value: string): string {
  if (/[\r\n\0]/.test(value)) {
    throw new TypeError("invalid middleware request header value");
  }
  return value;
}
