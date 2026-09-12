import { boundRequestBody, PayloadTooLargeError } from "./request-body.js";
import { scopedIdempotencyKey } from "./idempotency-scope.js";
import { currentContext, runWithContext } from "./context.js";
import {
  checkRequestContract,
  type RequestContractValidator
} from "./request-contract.js";
import {
  operationContextFromRequestContext,
  runOperationBoundary,
  type OperationFailure,
  type OperationFailureReporter
} from "./operation.js";

export { currentContext, runWithContext, checkRequestContract };
export type {
  RequestContractBody,
  RequestContractFailure,
  RequestContractIssue,
  RequestContractMatch,
  RequestContractValidationInput,
  RequestContractValidator
} from "./request-contract.js";

export const contractVersion = "1.0.0" as const;
export const capabilities = Object.freeze([
  "request-context", "panic-recovery", "request-id", "trace-context", "structured-logging", "metrics-red", "deadline-timeout", "payload-limit", "rate-limit", "auth", "sync-observer", "json", "headers", "compression", "tls-policy", "security-headers", "idempotency", "ip-policy", "cache-etag", "content-negotiation", "fault-injection", "test-auth-bypass", "schema-capture"
] as const);

export type Capability = (typeof capabilities)[number];
export type RuntimeEnvironment = "development" | "test" | "staging" | "production";
export type IntegrationMode = "disabled" | "http" | "embedded";

export interface RequestContext {
  requestId: string;
  traceId: string;
  spanId?: string;
  tenantId?: string;
  userId?: string;
  locale?: string;
  startedAtUnixMs: number;
  deadlineUnixMs?: number;
  baggage: Record<string, string>;
}

export interface MiddlewareConfig {
  contractVersion: string;
  environment: RuntimeEnvironment;
  requiredCapabilities: string[];
  settings: {
    requestIdHeader: string;
    traceHeader: string;
    timeoutMs: number;
    maxBodyBytes: number;
    contextRegistryMaxEntries: number;
    contextRegistryTtlMs: number;
    rateLimit: { enabled: boolean; capacity: number; refillPerSecond: number; keyBy: Array<"ip" | "user" | "tenant" | "route"> };
    compression: { enabled: boolean; minimumBytes: number; algorithms: string[] };
    tls: { mode: "disabled" | "in-process" | "trusted-proxy"; requireHttps: boolean; strictForwardedHeaders: boolean; trustedProxyCidrs: string[] };
    securityHeaders: { enabled: boolean; hstsMaxAgeSeconds: number; contentSecurityPolicy?: string; frameOptions: "DENY" | "SAMEORIGIN" };
    idempotency: { enabled: boolean; headerName: string; ttlSeconds: number; requiredMethods: string[] };
    faultInjection: { enabled: boolean; latencyMs: number; errorRate: number; dropRate: number };
    testAuthBypass: { enabled: boolean; headerName: string; allowedCidrs: string[] };
    contentRepresentations: string[];
  };
  integrations: {
    sharedAuth: { mode: IntegrationMode; issuer?: string; audience?: string; jwksUri?: string; introspectionUrl?: string; failOpen: boolean };
    optoSync: { mode: IntegrationMode; endpoint?: string; outboxTopic?: string; failOpen: boolean };
    oresOtel: { enabled: boolean; serviceName: string; exporterEndpoint?: string; propagators: string[] };
  };
}

export interface ValidationIssue { path: string; code: string; message: string }
export interface AuthDecision { userId?: string; tenantId?: string; claims?: Record<string, string> }
export interface StoredResponse { status: number; headers: Array<[string, string]>; body: Uint8Array; expiresAt: number }

export interface MiddlewareDependencies {
  authVerifier?: (request: Request, context: RequestContext) => Promise<AuthDecision>;
  resolveTestIdentity?: (request: Request, context: RequestContext) => Promise<AuthDecision>;
  rateLimiter?: { allow(key: string, capacity: number, refillPerSecond: number): Promise<boolean> };
  idempotencyStore?: { get(key: string): Promise<StoredResponse | undefined>; set(key: string, response: StoredResponse): Promise<void> };
  isTrustedProxy?: (request: Request) => boolean;
  authorizeIp?: (request: Request, context: RequestContext) => Promise<boolean>;
  telemetry?: {
    started(context: RequestContext, request: Request): Promise<void> | void;
    finished(context: RequestContext, request: Request, response: Response, durationMs: number): Promise<void> | void;
  };
  /** Optional audited sink; defaults to the bounded ores-otel reporter. */
  operationFailureReporter?: OperationFailureReporter;
  /**
   * Strict parsed-request contract boundary. Operation resolution receives
   * method + pathname only; path/query/header/body data is validation-only.
   */
  requestContractValidator?: RequestContractValidator;
  syncObserver?: (context: RequestContext, request: Request, response: Response, durationMs: number) => Promise<void>;
  captureSchema?: (request: Request, response: Response) => Promise<void>;
  now?: () => number;
  random?: () => number;
}

export type NextHandler = (request: Request) => Promise<Response>;
export type PortableMiddleware = (request: Request, next: NextHandler) => Promise<Response>;

export class MiddlewareConfigError extends Error {
  constructor(public readonly issues: ValidationIssue[]) {
    super(`invalid middleware configuration: ${issues.map((issue) => `${issue.path}:${issue.code}`).join(", ")}`);
  }
}

export function defaultConfig(serviceName: string): MiddlewareConfig {
  return {
    contractVersion,
    environment: "development",
    requiredCapabilities: [...capabilities],
    settings: {
      requestIdHeader: "x-request-id",
      traceHeader: "traceparent",
      timeoutMs: 5_000,
      maxBodyBytes: 2 * 1024 * 1024,
      contextRegistryMaxEntries: 10_000,
      contextRegistryTtlMs: 30_000,
      rateLimit: { enabled: true, capacity: 100, refillPerSecond: 20, keyBy: ["tenant", "user", "ip", "route"] },
      compression: { enabled: true, minimumBytes: 1_024, algorithms: ["br", "gzip"] },
      tls: { mode: "trusted-proxy", requireHttps: true, strictForwardedHeaders: true, trustedProxyCidrs: ["127.0.0.1/32", "::1/128"] },
      securityHeaders: { enabled: true, hstsMaxAgeSeconds: 31_536_000, contentSecurityPolicy: "default-src 'self'; frame-ancestors 'none'", frameOptions: "DENY" },
      idempotency: { enabled: true, headerName: "idempotency-key", ttlSeconds: 86_400, requiredMethods: ["POST", "PUT", "PATCH"] },
      faultInjection: { enabled: false, latencyMs: 0, errorRate: 0, dropRate: 0 },
      testAuthBypass: { enabled: false, headerName: "x-test-auth-bypass", allowedCidrs: ["127.0.0.1/32", "::1/128"] },
      contentRepresentations: ["application/json", "application/problem+json"]
    },
    integrations: {
      sharedAuth: { mode: "disabled", failOpen: false },
      optoSync: { mode: "disabled", failOpen: true },
      oresOtel: { enabled: true, serviceName, propagators: ["tracecontext", "baggage"] }
    }
  };
}

/** A pure validation rule: config in, zero or more issues out. */
type ConfigRule = (config: MiddlewareConfig) => readonly ValidationIssue[];

const issueWhen = (failed: boolean, path: string, code: string, message: string): readonly ValidationIssue[] =>
  failed ? [{ path, code, message }] : [];

/**
 * Rules are independent values composed by `validateConfig`; none of them
 * shares or mutates an accumulator, so each can be read and tested alone.
 */
const configRules: readonly ConfigRule[] = [
  (c) => issueWhen(c.contractVersion !== contractVersion, "/contractVersion", "unsupported_version", `expected ${contractVersion}`),
  (c) => issueWhen(!Number.isFinite(c.settings.timeoutMs) || c.settings.timeoutMs <= 0, "/settings/timeoutMs", "range", "timeout must be positive"),
  (c) => issueWhen(!Number.isSafeInteger(c.settings.maxBodyBytes) || c.settings.maxBodyBytes <= 0, "/settings/maxBodyBytes", "range", "body limit must be a positive safe integer"),
  (c) => issueWhen(c.settings.rateLimit.enabled && (c.settings.rateLimit.capacity <= 0 || c.settings.rateLimit.refillPerSecond <= 0), "/settings/rateLimit", "invalid_rate_limit", "enabled token bucket requires positive capacity and refill"),
  (c) => issueWhen(c.settings.faultInjection.errorRate < 0 || c.settings.faultInjection.errorRate > 1 || c.settings.faultInjection.dropRate < 0 || c.settings.faultInjection.dropRate > 1, "/settings/faultInjection", "range", "fault rates must be within 0..=1"),
  (c) => issueWhen(c.environment === "production" && c.settings.faultInjection.enabled, "/settings/faultInjection/enabled", "production_forbidden", "fault injection is forbidden in production"),
  (c) => issueWhen(c.environment === "production" && c.settings.testAuthBypass.enabled, "/settings/testAuthBypass/enabled", "production_forbidden", "test auth bypass is forbidden in production"),
  (c) => issueWhen(c.integrations.sharedAuth.failOpen, "/integrations/sharedAuth/failOpen", "auth_fail_open", "shared-auth must fail closed"),
  (c) => issueWhen(c.settings.tls.mode === "trusted-proxy" && c.settings.tls.trustedProxyCidrs.length === 0, "/settings/tls/trustedProxyCidrs", "trusted_proxy_required", "trusted-proxy mode requires explicit CIDRs"),
  (c) =>
    c.requiredCapabilities
      .filter((capability) => !(capabilities as readonly string[]).includes(capability))
      .map((capability) => ({ path: "/requiredCapabilities", code: "unknown_capability", message: capability }))
];

export function validateConfig(config: MiddlewareConfig): ValidationIssue[] {
  return configRules.flatMap((rule) => rule(config));
}

class MemoryTokenBucket {
  readonly #buckets = new Map<string, { tokens: number; last: number }>();
  constructor(private readonly now: () => number) {}
  async allow(key: string, capacity: number, refillPerSecond: number): Promise<boolean> {
    const now = this.now();
    const previous = this.#buckets.get(key) ?? { tokens: capacity, last: now };
    // The bucket is a new value derived from the previous one; the Map is the
    // one stateful store and is replaced entry-by-entry, never edited in place.
    const refilled = Math.min(capacity, previous.tokens + ((now - previous.last) / 1_000) * refillPerSecond);
    const allowed = refilled >= 1;
    this.#buckets.set(key, { tokens: allowed ? refilled - 1 : refilled, last: now });
    return allowed;
  }
}

class MemoryIdempotencyStore {
  readonly #entries = new Map<string, StoredResponse>();
  constructor(private readonly now: () => number) {}
  async get(key: string): Promise<StoredResponse | undefined> {
    const value = this.#entries.get(key);
    if (value && value.expiresAt > this.now()) return value;
    if (value) this.#entries.delete(key);
    return undefined;
  }
  async set(key: string, value: StoredResponse): Promise<void> { this.#entries.set(key, value); }
}

/** The post-authentication request context, built as a new value from the pre-auth one. */
function withAuth(context: RequestContext, auth: AuthDecision): RequestContext {
  return {
    ...context,
    userId: auth.userId,
    tenantId: auth.tenantId,
    baggage: {
      ...context.baggage,
      ...Object.fromEntries(Object.entries(auth.claims ?? {}).filter(([key]) => key.startsWith("otel.")))
    }
  };
}

export function createMiddleware(config: MiddlewareConfig, dependencies: MiddlewareDependencies = {}): PortableMiddleware {
  const issues = validateConfig(config);
  if (issues.length > 0) throw new MiddlewareConfigError(issues);
  const now = dependencies.now ?? Date.now;
  const random = dependencies.random ?? Math.random;
  const rateLimiter = dependencies.rateLimiter ?? new MemoryTokenBucket(now);
  const idempotencyStore = dependencies.idempotencyStore ?? new MemoryIdempotencyStore(now);

  return async (request, next) => {
    const started = now();
    const requestId = validToken(request.headers.get(config.settings.requestIdHeader)) ?? crypto.randomUUID();
    const traceId = parseTraceId(request.headers.get(config.settings.traceHeader)) ?? crypto.randomUUID().replaceAll("-", "");
    const initialContext: RequestContext = {
      requestId,
      traceId,
      locale: request.headers.get("accept-language") ?? undefined,
      startedAtUnixMs: started,
      deadlineUnixMs: started + config.settings.timeoutMs,
      baggage: {}
    };

    const preAuthOutcome = await runOperationBoundary(
      { transport: "http", scope: "request", name: "middleware.pre_auth", signal: request.signal },
      async () => {
    // Every early exit carries the pre-auth context; the success path carries the
    // authenticated one, so nothing outside this closure depends on a mutated binding.
    const early = (response: Response) => ({ response, context: initialContext });
    const contentLength = Number(request.headers.get("content-length") ?? "0");
    if (Number.isFinite(contentLength) && contentLength > config.settings.maxBodyBytes) return early(problem(413, "payload_too_large", "request body exceeds configured limit"));
    const accepted = request.headers.get("accept");
    if (accepted && accepted !== "*/*" && !config.settings.contentRepresentations.some((representation) => accepted.includes(representation))) return early(problem(406, "not_acceptable", "no supported representation was requested"));

    const url = new URL(request.url);
    const forwardedProto = request.headers.get("x-forwarded-proto");
    const trustedProxy = dependencies.isTrustedProxy?.(request) ?? false;
    const effectiveHttps = url.protocol === "https:" || (trustedProxy && forwardedProto === "https");
    if (config.settings.tls.requireHttps && !effectiveHttps) return early(problem(426, "https_required", "HTTPS is required"));
    if (config.settings.tls.strictForwardedHeaders && forwardedProto && !trustedProxy) return early(problem(400, "untrusted_forwarded_header", "forwarded transport headers came from an untrusted peer"));

    if (dependencies.authorizeIp && !(await dependencies.authorizeIp(request, initialContext))) return early(problem(403, "ip_policy_denied", "request source is not permitted"));

    try {
      request = await boundRequestBody(request, config.settings.maxBodyBytes, config.settings.timeoutMs);
    } catch (error) {
      if (error instanceof PayloadTooLargeError) return problem(413, "payload_too_large", "request body exceeds configured limit");
      throw error;
    }

    const contractFailure = await checkRequestContract(
      dependencies.requestContractValidator,
      request,
      url
    );
    if (contractFailure) {
      const detail = contractFailure.code === "unknown_operation"
        ? "no request contract matched the HTTP method and pathname"
        : "request path, query, headers, or JSON payload failed contract validation";
      return early(problem(contractFailure.status, contractFailure.code, detail));
    }

    if (config.settings.rateLimit.enabled) {
      const rateKey = [initialContext.tenantId ?? "_", initialContext.userId ?? "_", request.headers.get("x-real-ip") ?? "_", url.pathname].join(":");
      if (!(await rateLimiter.allow(rateKey, config.settings.rateLimit.capacity, config.settings.rateLimit.refillPerSecond))) return early(problem(429, "rate_limited", "rate limit exceeded"));
    }

    const canBypass = config.environment === "test" || config.environment === "staging";
    const bypassRequested = config.settings.testAuthBypass.enabled && request.headers.get(config.settings.testAuthBypass.headerName) === "true";
    if (bypassRequested && (!canBypass || !dependencies.resolveTestIdentity)) return early(problem(403, "test_bypass_denied", "test identity bypass is unavailable"));
    const auth: AuthDecision = bypassRequested
      ? await dependencies.resolveTestIdentity!(request, initialContext)
      : dependencies.authVerifier
        ? await dependencies.authVerifier(request, initialContext)
        : {};
    // The authenticated context is a new object; the pre-auth context is never edited.
    const context: RequestContext = withAuth(initialContext, auth);

    const authenticatedOutcome = await runOperationBoundary(
      { transport: "http", scope: "request", name: "middleware.request", signal: request.signal },
      async () => {
    if (config.integrations.sharedAuth.mode !== "disabled" && !context.userId) return problem(401, "authentication_required", "shared-auth did not establish a user");

    if (config.settings.faultInjection.enabled) {
      if (config.settings.faultInjection.latencyMs > 0) await delay(config.settings.faultInjection.latencyMs);
      if (random() < config.settings.faultInjection.dropRate) return problem(503, "fault_drop", "injected transport drop");
      if (random() < config.settings.faultInjection.errorRate) return problem(500, "fault_error", "injected middleware error");
    }

    const idempotencyKey = config.settings.idempotency.enabled && config.settings.idempotency.requiredMethods.includes(request.method.toUpperCase()) ? request.headers.get(config.settings.idempotency.headerName) : null;
    const replayKey = idempotencyKey ? await scopedIdempotencyKey({
      serviceName: config.integrations.oresOtel.serviceName,
      tenantId: context.tenantId ?? "", userId: context.userId ?? "",
      method: request.method, path: url.pathname, query: url.search.slice(1), idempotencyKey
    }) : undefined;
    if (replayKey) {
      const cached = await idempotencyStore.get(replayKey);
      if (cached) return new Response(cached.body.slice(), { status: cached.status, headers: cached.headers });
    }

    await dependencies.telemetry?.started(context, request);
    const response = maybeCompress(
      config,
      request,
      await attachEtag(request, await withDeadline(config.settings.timeoutMs, () => next(request)))
    );
    // Observers and persistence consume the semantic response before
    // request-specific security and correlation headers are finalized.
    const durationMs = Math.max(0, now() - started);
    await dependencies.telemetry?.finished(context, request, response.clone(), durationMs);
    if (dependencies.captureSchema) await dependencies.captureSchema(request.clone(), response.clone());
    if (dependencies.syncObserver) {
      try { await dependencies.syncObserver(context, request.clone(), response.clone(), durationMs); }
      catch { if (!config.integrations.optoSync.failOpen) return problem(503, "sync_observer_failed", "opto-sync observation failed"); }
    }

    if (replayKey && response.status >= 200 && response.status < 300) {
      const body = new Uint8Array(await response.clone().arrayBuffer());
      await idempotencyStore.set(replayKey, { status: response.status, headers: [...response.headers.entries()], body, expiresAt: now() + config.settings.idempotency.ttlSeconds * 1_000 });
    }
    return response;
      },
      {
        context: operationContextFromRequestContext(context),
        reportFailure: dependencies.operationFailureReporter
      }
    );
    const response = authenticatedOutcome.ok
      ? authenticatedOutcome.value
      : operationFailureResponse(authenticatedOutcome.failure);
    return { response, context };
      },
      {
        context: operationContextFromRequestContext(initialContext),
        reportFailure: dependencies.operationFailureReporter
      }
    );
    const { response, context: finalContext } = preAuthOutcome.ok
      ? preAuthOutcome.value
      : { response: operationFailureResponse(preAuthOutcome.failure), context: initialContext };
    return attachHeaders(config, finalContext, response);
  };
}

export async function readJson<T>(request: Request, validator?: (value: unknown) => value is T): Promise<T> {
  const contentType = request.headers.get("content-type") ?? "";
  if (!contentType.toLowerCase().includes("application/json")) throw new TypeError("expected application/json");
  const value: unknown = await request.json();
  if (validator && !validator(value)) throw new TypeError("JSON body failed validation");
  return value as T;
}

export function sharedAuthHttpVerifier(config: MiddlewareConfig["integrations"]["sharedAuth"]): NonNullable<MiddlewareDependencies["authVerifier"]> {
  if (!config.introspectionUrl) throw new Error("shared-auth introspectionUrl is required for HTTP mode");
  return async (request) => {
    const authorization = request.headers.get("authorization");
    if (!authorization) return {};
    const response = await fetch(config.introspectionUrl!, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ authorization, audience: config.audience }) });
    if (!response.ok) throw new Error("shared-auth introspection failed");
    const payload = await response.json() as { active?: boolean; sub?: string; tenantId?: string; claims?: Record<string, string> };
    if (!payload.active || !payload.sub) return {};
    return { userId: payload.sub, tenantId: payload.tenantId, claims: payload.claims };
  };
}

export function optoSyncHttpObserver(config: MiddlewareConfig["integrations"]["optoSync"]): NonNullable<MiddlewareDependencies["syncObserver"]> {
  if (!config.endpoint) throw new Error("opto-sync endpoint is required for HTTP mode");
  return async (context, request, response, durationMs) => {
    const result = await fetch(config.endpoint!, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ topic: config.outboxTopic, requestId: context.requestId, traceId: context.traceId, method: request.method, path: new URL(request.url).pathname, status: response.status, durationMs }) });
    if (!result.ok) throw new Error(`opto-sync returned ${result.status}`);
  };
}

export function descriptor() {
  return {
    contractVersion,
    language: "ts",
    runtime: "node-deno-bun",
    packageName: "@oresoftware/ores-middleware",
    frameworkAdapters: ["express", "koa", "fastify", "deno", "bun", "nestjs", "nextjs", "nuxt", "hapi", "hono", "node-http"],
    capabilities: [...capabilities],
    operationSymbols: { descriptor: "descriptor", defaultConfig: "defaultConfig", validateConfig: "validateConfig", createMiddleware: "createMiddleware", runWithContext: "runWithContext", currentContext: "currentContext", capabilities: "capabilities" }
  };
}

class DeadlineError extends Error {
  constructor() {
    super("request deadline exceeded");
    this.name = "TimeoutError";
  }
}
function withDeadline<T>(timeoutMs: number, operation: () => Promise<T>): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new DeadlineError()), timeoutMs);
    operation().then(resolve, reject).finally(() => clearTimeout(timer));
  });
}
function delay(ms: number): Promise<void> { return new Promise((resolve) => setTimeout(resolve, ms)); }
function validToken(value: string | null): string | undefined { return value && value.length <= 128 && /^[A-Za-z0-9._-]+$/.test(value) ? value : undefined; }
function parseTraceId(value: string | null): string | undefined {
  const part = value?.split("-")[1]?.toLowerCase();
  return part &&
    /^[0-9a-f]{32}$/.test(part) &&
    part !== "00000000000000000000000000000000"
    ? part
    : undefined;
}

function validTraceparent(value: string | null): string | undefined {
  if (!value) return undefined;
  const parts = value.split("-");
  if (parts.length !== 4) return undefined;

  const version = parts[0]?.toLowerCase();
  const traceId = parts[1]?.toLowerCase();
  const spanId = parts[2]?.toLowerCase();
  const flags = parts[3]?.toLowerCase();
  if (
    version !== "00" ||
    !traceId ||
    !/^[0-9a-f]{32}$/.test(traceId) ||
    traceId === "00000000000000000000000000000000" ||
    !spanId ||
    !/^[0-9a-f]{16}$/.test(spanId) ||
    spanId === "0000000000000000" ||
    !flags ||
    !/^[0-9a-f]{2}$/.test(flags)
  ) {
    return undefined;
  }
  return `${version}-${traceId}-${spanId}-${flags}`;
}

function operationFailureResponse(failure: OperationFailure): Response {
  return failure.kind === "deadline_exceeded"
    ? problem(504, "deadline_exceeded", "request deadline exceeded")
    : failure.kind === "cancelled"
      ? problem(499, "request_cancelled", "request was cancelled")
      : problem(500, "internal_error", "request processing failed");
}

function problem(status: number, code: string, detail: string): Response { return Response.json({ type: `urn:ores:middleware:${code}`, title: code, status, detail }, { status, headers: { "content-type": "application/problem+json" } }); }
function mergeVary(headers: Headers, ...tokens: string[]): void {
  const existing = headers.get("vary");
  if (existing?.trim() === "*") return;
  const values = new Map<string, string>();
  for (const value of [...(existing?.split(",") ?? []), ...tokens]) {
    const trimmed = value.trim();
    if (!trimmed) continue;
    const key = trimmed.toLowerCase();
    if (!values.has(key)) values.set(key, key);
  }
  if (values.size > 0) headers.set("vary", [...values.values()].join(", "));
  else headers.delete("vary");
}
function attachHeaders(config: MiddlewareConfig, context: RequestContext, response: Response): Response {
  const headers = new Headers(response.headers);
  headers.set(config.settings.requestIdHeader, context.requestId);
  const responseTraceparent = validTraceparent(headers.get("traceparent"));
  if (responseTraceparent) headers.set("traceparent", responseTraceparent);
  else headers.delete("traceparent");
  mergeVary(headers, "accept", "accept-encoding");
  if (config.settings.securityHeaders.enabled) {
    headers.set("x-content-type-options", "nosniff"); headers.set("x-frame-options", config.settings.securityHeaders.frameOptions); headers.set("referrer-policy", "strict-origin-when-cross-origin"); headers.set("strict-transport-security", `max-age=${config.settings.securityHeaders.hstsMaxAgeSeconds}; includeSubDomains`);
    if (config.settings.securityHeaders.contentSecurityPolicy) headers.set("content-security-policy", config.settings.securityHeaders.contentSecurityPolicy);
  }
  return new Response(response.body, { status: response.status, statusText: response.statusText, headers });
}
async function attachEtag(request: Request, response: Response): Promise<Response> {
  if (request.method !== "GET" || response.status !== 200 || response.headers.has("etag") || !response.body) return response;
  const body = new Uint8Array(await response.arrayBuffer());
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", body));
  const etag = `"${[...digest].map((value) => value.toString(16).padStart(2, "0")).join("")}"`;
  if (request.headers.get("if-none-match") === etag) return new Response(null, { status: 304, headers: { etag } });
  const headers = new Headers(response.headers); headers.set("etag", etag);
  return new Response(body, { status: response.status, statusText: response.statusText, headers });
}
function maybeCompress(config: MiddlewareConfig, request: Request, response: Response): Response {
  if (!config.settings.compression.enabled || !response.body || typeof CompressionStream === "undefined") return response;
  const length = Number(response.headers.get("content-length") ?? "0");
  if (!Number.isFinite(length) || length < config.settings.compression.minimumBytes || !request.headers.get("accept-encoding")?.includes("gzip")) return response;
  const headers = new Headers(response.headers); headers.delete("content-length"); headers.set("content-encoding", "gzip"); mergeVary(headers, "accept-encoding");
  return new Response(response.body.pipeThrough(new CompressionStream("gzip")), { status: response.status, statusText: response.statusText, headers });
}
