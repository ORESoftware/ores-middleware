import { currentContext, runWithContext } from "./context.js";
import {
  operationContextFromRequestContext,
  runOperationBoundary,
  type OperationFailure,
  type OperationFailureReporter
} from "./operation.js";

export { currentContext, runWithContext };

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

export function validateConfig(config: MiddlewareConfig): ValidationIssue[] {
  const issues: ValidationIssue[] = [];
  const issue = (path: string, code: string, message: string) => issues.push({ path, code, message });
  if (config.contractVersion !== contractVersion) issue("/contractVersion", "unsupported_version", `expected ${contractVersion}`);
  if (!Number.isFinite(config.settings.timeoutMs) || config.settings.timeoutMs <= 0) issue("/settings/timeoutMs", "range", "timeout must be positive");
  if (!Number.isSafeInteger(config.settings.maxBodyBytes) || config.settings.maxBodyBytes <= 0) issue("/settings/maxBodyBytes", "range", "body limit must be a positive safe integer");
  if (config.settings.rateLimit.enabled && (config.settings.rateLimit.capacity <= 0 || config.settings.rateLimit.refillPerSecond <= 0)) issue("/settings/rateLimit", "invalid_rate_limit", "enabled token bucket requires positive capacity and refill");
  if (config.settings.faultInjection.errorRate < 0 || config.settings.faultInjection.errorRate > 1 || config.settings.faultInjection.dropRate < 0 || config.settings.faultInjection.dropRate > 1) issue("/settings/faultInjection", "range", "fault rates must be within 0..=1");
  if (config.environment === "production" && config.settings.faultInjection.enabled) issue("/settings/faultInjection/enabled", "production_forbidden", "fault injection is forbidden in production");
  if (config.environment === "production" && config.settings.testAuthBypass.enabled) issue("/settings/testAuthBypass/enabled", "production_forbidden", "test auth bypass is forbidden in production");
  if (config.integrations.sharedAuth.failOpen) issue("/integrations/sharedAuth/failOpen", "auth_fail_open", "shared-auth must fail closed");
  if (config.settings.tls.mode === "trusted-proxy" && config.settings.tls.trustedProxyCidrs.length === 0) issue("/settings/tls/trustedProxyCidrs", "trusted_proxy_required", "trusted-proxy mode requires explicit CIDRs");
  for (const capability of config.requiredCapabilities) if (!(capabilities as readonly string[]).includes(capability)) issue("/requiredCapabilities", "unknown_capability", capability);
  return issues;
}

class MemoryTokenBucket {
  readonly #buckets = new Map<string, { tokens: number; last: number }>();
  constructor(private readonly now: () => number) {}
  async allow(key: string, capacity: number, refillPerSecond: number): Promise<boolean> {
    const now = this.now();
    const bucket = this.#buckets.get(key) ?? { tokens: capacity, last: now };
    bucket.tokens = Math.min(capacity, bucket.tokens + ((now - bucket.last) / 1_000) * refillPerSecond);
    bucket.last = now;
    const allowed = bucket.tokens >= 1;
    if (allowed) bucket.tokens -= 1;
    this.#buckets.set(key, bucket);
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
    let context: RequestContext = {
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
    const contentLength = Number(request.headers.get("content-length") ?? "0");
    if (Number.isFinite(contentLength) && contentLength > config.settings.maxBodyBytes) return problem(413, "payload_too_large", "request body exceeds configured limit");
    const accepted = request.headers.get("accept");
    if (accepted && accepted !== "*/*" && !config.settings.contentRepresentations.some((representation) => accepted.includes(representation))) return problem(406, "not_acceptable", "no supported representation was requested");

    const url = new URL(request.url);
    const forwardedProto = request.headers.get("x-forwarded-proto");
    const trustedProxy = dependencies.isTrustedProxy?.(request) ?? false;
    const effectiveHttps = url.protocol === "https:" || (trustedProxy && forwardedProto === "https");
    if (config.settings.tls.requireHttps && !effectiveHttps) return problem(426, "https_required", "HTTPS is required");
    if (config.settings.tls.strictForwardedHeaders && forwardedProto && !trustedProxy) return problem(400, "untrusted_forwarded_header", "forwarded transport headers came from an untrusted peer");

    if (dependencies.authorizeIp && !(await dependencies.authorizeIp(request, context))) return problem(403, "ip_policy_denied", "request source is not permitted");
    if (config.settings.rateLimit.enabled) {
      const rateKey = [context.tenantId ?? "_", context.userId ?? "_", request.headers.get("x-real-ip") ?? "_", url.pathname].join(":");
      if (!(await rateLimiter.allow(rateKey, config.settings.rateLimit.capacity, config.settings.rateLimit.refillPerSecond))) return problem(429, "rate_limited", "rate limit exceeded");
    }

    const canBypass = config.environment === "test" || config.environment === "staging";
    const bypassRequested = config.settings.testAuthBypass.enabled && request.headers.get(config.settings.testAuthBypass.headerName) === "true";
    let auth: AuthDecision = {};
    if (bypassRequested) {
      if (!canBypass || !dependencies.resolveTestIdentity) return problem(403, "test_bypass_denied", "test identity bypass is unavailable");
      auth = await dependencies.resolveTestIdentity(request, context);
    } else if (dependencies.authVerifier) {
      auth = await dependencies.authVerifier(request, context);
    }
    context = {
      ...context,
      userId: auth.userId,
      tenantId: auth.tenantId,
      baggage: {
        ...context.baggage,
        ...Object.fromEntries(
          Object.entries(auth.claims ?? {}).filter(([key]) => key.startsWith("otel."))
        )
      }
    };

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
    if (idempotencyKey) {
      const cached = await idempotencyStore.get(`${request.method}:${url.pathname}:${idempotencyKey}`);
      if (cached) return new Response(cached.body.slice(), { status: cached.status, headers: cached.headers });
    }

    await dependencies.telemetry?.started(context, request);
    let response = await withDeadline(config.settings.timeoutMs, () => next(request));

    response = await attachEtag(request, response);
    response = maybeCompress(config, request, response);
    // Observers and idempotency storage receive the semantic response before
    // request-specific correlation/security headers are added. The one outer
    // finalizer applies those headers exactly once to every response path.
    const durationMs = Math.max(0, now() - started);
    await dependencies.telemetry?.finished(context, request, response.clone(), durationMs);
    if (dependencies.captureSchema) await dependencies.captureSchema(request.clone(), response.clone());
    if (dependencies.syncObserver) {
      try { await dependencies.syncObserver(context, request.clone(), response.clone(), durationMs); }
      catch { if (!config.integrations.optoSync.failOpen) return problem(503, "sync_observer_failed", "opto-sync observation failed"); }
    }

    if (idempotencyKey && response.status >= 200 && response.status < 300) {
      const body = new Uint8Array(await response.clone().arrayBuffer());
      await idempotencyStore.set(`${request.method}:${url.pathname}:${idempotencyKey}`, { status: response.status, headers: [...response.headers.entries()], body, expiresAt: now() + config.settings.idempotency.ttlSeconds * 1_000 });
    }
    return response;
      },
      {
        context: operationContextFromRequestContext(context),
        reportFailure: dependencies.operationFailureReporter
      }
    );
    return authenticatedOutcome.ok
      ? authenticatedOutcome.value
      : operationFailureResponse(authenticatedOutcome.failure);
      },
      {
        context: operationContextFromRequestContext(context),
        reportFailure: dependencies.operationFailureReporter
      }
    );
    const response = preAuthOutcome.ok
      ? preAuthOutcome.value
      : operationFailureResponse(preAuthOutcome.failure);
    return attachHeaders(config, context, response);
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
    frameworkAdapters: ["express", "deno", "bun", "nestjs", "nextjs", "nuxt", "hapi", "hono", "node-http"],
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
  const values = new Set<string>();
  for (const value of [...(existing?.split(",") ?? []), ...tokens]) {
    const token = value.trim().toLowerCase();
    if (token) values.add(token);
  }
  if (values.size > 0) headers.set("vary", [...values].join(", "));
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
