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
  syncObserver?: (context: RequestContext, request: Request, response: Response, durationMs: number) => Promise<void>;
  telemetry?: { started(context: RequestContext, request: Request): void; finished(context: RequestContext, request: Request, response: Response, durationMs: number): void };
  schemaCapture?: (request: Request, response: Response) => Promise<void>;
  /** Optional audited sink; defaults to the bounded ores-otel reporter. */
  operationFailureReporter?: OperationFailureReporter;
  /**
   * Strict parsed-request contract boundary. Its resolver receives method +
   * pathname only; query/header/body values are validation-only inputs.
   */
  requestContractValidator?: RequestContractValidator;
  /**
   * Optional callback that observes each rejected request after finalization.
   * The middleware preserves the original status and body even if this hook
   * throws; implementations must not log credentials or request bodies.
   */
  rejectionObserver?: (event: RejectionObservation) => void | Promise<void>;
}

export interface RejectionObservation {
  readonly requestId: string;
  readonly traceId: string;
  readonly method: string;
  readonly pathname: string;
  readonly status: number;
  readonly code: string;
  readonly durationMs: number;
}

export type Next = (request: Request) => Promise<Response>;
export type Middleware = (request: Request, next: Next) => Promise<Response>;
export type PortableMiddleware = Middleware;

export function defaultConfig(serviceName: string): MiddlewareConfig {
  return {
    contractVersion,
    environment: "development",
    requiredCapabilities: [...capabilities],
    settings: {
      requestIdHeader: "x-request-id", traceHeader: "traceparent", timeoutMs: 5000, maxBodyBytes: 2 * 1024 * 1024,
      contextRegistryMaxEntries: 10000, contextRegistryTtlMs: 30000,
      rateLimit: { enabled: true, capacity: 100, refillPerSecond: 20, keyBy: ["tenant", "user", "ip", "route"] },
      compression: { enabled: true, minimumBytes: 1024, algorithms: ["gzip"] },
      tls: { mode: "trusted-proxy", requireHttps: true, strictForwardedHeaders: true, trustedProxyCidrs: ["127.0.0.1/32", "::1/128"] },
      securityHeaders: { enabled: true, hstsMaxAgeSeconds: 31536000, contentSecurityPolicy: "default-src 'self'; frame-ancestors 'none'", frameOptions: "DENY" },
      idempotency: { enabled: true, headerName: "idempotency-key", ttlSeconds: 86400, requiredMethods: ["POST", "PUT", "PATCH"] },
      faultInjection: { enabled: false, latencyMs: 0, errorRate: 0, dropRate: 0 },
      testAuthBypass: { enabled: false, headerName: "x-test-auth-bypass", allowedCidrs: ["127.0.0.1/32", "::1/128"] },
      contentRepresentations: ["application/json", "application/problem+json"]
    },
    integrations: {
      sharedAuth: { mode: "disabled", failOpen: false }, optoSync: { mode: "disabled", failOpen: true },
      oresOtel: { enabled: true, serviceName, propagators: ["tracecontext", "baggage"] }
    }
  };
}

export function validateConfig(config: MiddlewareConfig): ValidationIssue[] {
  const issues: ValidationIssue[] = [];
  const add = (condition: boolean, path: string, code: string, message: string) => { if (condition) issues.push({ path, code, message }); };
  add(config.contractVersion !== contractVersion, "/contractVersion", "unsupported_version", `expected ${contractVersion}`);
  add(config.settings.timeoutMs <= 0, "/settings/timeoutMs", "range", "timeout must be positive");
  add(config.settings.maxBodyBytes <= 0, "/settings/maxBodyBytes", "range", "body limit must be positive");
  add(config.settings.rateLimit.enabled && (config.settings.rateLimit.capacity <= 0 || config.settings.rateLimit.refillPerSecond <= 0), "/settings/rateLimit", "invalid_rate_limit", "enabled token bucket requires positive capacity and refill");
  add(config.settings.faultInjection.errorRate < 0 || config.settings.faultInjection.errorRate > 1 || config.settings.faultInjection.dropRate < 0 || config.settings.faultInjection.dropRate > 1, "/settings/faultInjection", "range", "fault rates must be within 0..=1");
  add(config.environment === "production" && config.settings.faultInjection.enabled, "/settings/faultInjection/enabled", "production_forbidden", "fault injection is forbidden in production");
  add(config.environment === "production" && config.settings.testAuthBypass.enabled, "/settings/testAuthBypass/enabled", "production_forbidden", "test auth bypass is forbidden in production");
  add(config.integrations.sharedAuth.failOpen, "/integrations/sharedAuth/failOpen", "auth_fail_open", "shared-auth must fail closed");
  add(config.settings.tls.mode === "trusted-proxy" && config.settings.tls.trustedProxyCidrs.length === 0, "/settings/tls/trustedProxyCidrs", "trusted_proxy_required", "trusted-proxy mode requires explicit CIDRs");
  for (const required of config.requiredCapabilities) add(!capabilities.includes(required as Capability), "/requiredCapabilities", "unknown_capability", required);
  return issues;
}

function newId(): string { return crypto.randomUUID().replaceAll("-", ""); }
function headerToken(input: string | null, fallback: () => string): string { return input && /^[A-Za-z0-9._-]{1,128}$/.test(input) ? input : fallback(); }
function traceId(input: string | null): string {
  if (input) { const parts = input.split("-"); if (parts.length >= 4 && /^[0-9a-f]{32}$/i.test(parts[1])) return parts[1].toLowerCase(); }
  return newId();
}
function clientIp(request: Request): string { return request.headers.get("x-forwarded-for")?.split(",")[0]?.trim() || request.headers.get("x-real-ip") || "unknown"; }
function mediaAccepted(accept: string | null, supported: string[]): boolean {
  if (!accept || accept.includes("*/*")) return true;
  return supported.some((type) => accept.split(",").some((part) => part.split(";")[0].trim() === type));
}
function problem(status: number, code: string, detail: string): Response {
  return Response.json({ type: `urn:ores:middleware:${code}`, title: code, status, detail }, { status, headers: { "content-type": "application/problem+json" } });
}
function requestSize(request: Request): number { const value = request.headers.get("content-length"); return value ? Number(value) : 0; }
function rateKey(config: MiddlewareConfig, context: RequestContext, request: Request): string {
  const url = new URL(request.url); const values: string[] = [];
  for (const key of config.settings.rateLimit.keyBy) values.push(key === "tenant" ? context.tenantId || "_" : key === "user" ? context.userId || "_" : key === "ip" ? clientIp(request) : url.pathname);
  return values.join(":");
}
function applySecurityHeaders(config: MiddlewareConfig, headers: Headers): void {
  if (!config.settings.securityHeaders.enabled) return;
  headers.set("x-content-type-options", "nosniff"); headers.set("x-frame-options", config.settings.securityHeaders.frameOptions); headers.set("referrer-policy", "strict-origin-when-cross-origin");
  headers.set("strict-transport-security", `max-age=${config.settings.securityHeaders.hstsMaxAgeSeconds}; includeSubDomains`);
  if (config.settings.securityHeaders.contentSecurityPolicy) headers.set("content-security-policy", config.settings.securityHeaders.contentSecurityPolicy);
}
function appendVary(headers: Headers, value: string): void {
  const existing = headers.get("vary"); const set = new Set((existing || "").split(",").map((item) => item.trim().toLowerCase()).filter(Boolean)); set.add(value.toLowerCase()); headers.set("vary", [...set].join(", "));
}
function attachHeaders(config: MiddlewareConfig, context: RequestContext, response: Response): Response {
  const headers = new Headers(response.headers);
  headers.set(config.settings.requestIdHeader, context.requestId);
  if (context.spanId && !/^0{16}$/.test(context.spanId)) {
    headers.set("traceparent", `00-${context.traceId}-${context.spanId}-01`);
  } else {
    headers.delete("traceparent");
  }
  appendVary(headers, "accept"); appendVary(headers, "accept-encoding"); applySecurityHeaders(config, headers);
  return new Response(response.body, { status: response.status, statusText: response.statusText, headers });
}
async function maybeCompress(config: MiddlewareConfig, request: Request, response: Response): Promise<Response> {
  if (!config.settings.compression.enabled || response.headers.has("content-encoding") || !request.headers.get("accept-encoding")?.includes("gzip")) return response;
  const body = new Uint8Array(await response.clone().arrayBuffer()); if (body.length < config.settings.compression.minimumBytes) return response;
  if (typeof CompressionStream === "undefined") return response; const stream = new Blob([body]).stream().pipeThrough(new CompressionStream("gzip"));
  const headers = new Headers(response.headers); headers.set("content-encoding", "gzip"); appendVary(headers, "accept-encoding"); return new Response(stream, { status: response.status, headers });
}
async function handlerWithTimeout(next: Next, request: Request, timeoutMs: number, signal?: AbortSignal): Promise<Response> {
  const controller = new AbortController(); let timeoutHandle: ReturnType<typeof setTimeout> | undefined;
  let rejectAbort: ((reason?: unknown) => void) | undefined;
  const cancellation = new Promise<never>((_resolve, reject) => { rejectAbort = reject; });
  const abort = () => { controller.abort(signal?.reason); rejectAbort?.(signal?.reason ?? new DOMException("request cancelled", "AbortError")); };
  if (signal?.aborted) abort(); else signal?.addEventListener("abort", abort, { once: true });
  try {
    const timeout = new Promise<never>((_resolve, reject) => { timeoutHandle = setTimeout(() => { controller.abort(); reject(new Error("deadline exceeded")); }, timeoutMs); });
    const scoped = new Request(request, { signal: controller.signal }); return await Promise.race([next(scoped), timeout, cancellation]);
  } finally { if (timeoutHandle) clearTimeout(timeoutHandle); signal?.removeEventListener("abort", abort); }
}
function idempotencyKey(config: MiddlewareConfig, request: Request): string | undefined {
  if (!config.settings.idempotency.enabled || !config.settings.idempotency.requiredMethods.includes(request.method)) return undefined;
  const value = request.headers.get(config.settings.idempotency.headerName); if (!value) return undefined; const url = new URL(request.url); return `${request.method}:${url.pathname}:${value}`;
}
async function storedResponse(response: Response, ttlSeconds: number): Promise<StoredResponse> {
  return { status: response.status, headers: [...response.headers.entries()], body: new Uint8Array(await response.clone().arrayBuffer()), expiresAt: Date.now() + ttlSeconds * 1000 };
}
function fromStored(value: StoredResponse): Response { return new Response(value.body, { status: value.status, headers: value.headers }); }
function maybeTestIdentity(config: MiddlewareConfig, request: Request, dependencies: MiddlewareDependencies, context: RequestContext): Promise<AuthDecision> | undefined {
  const enabled = config.settings.testAuthBypass.enabled && request.headers.get(config.settings.testAuthBypass.headerName) === "true";
  if (!enabled) return undefined;
  if (config.environment !== "test" && config.environment !== "staging") return Promise.reject(new Error("test auth bypass is forbidden outside test/staging"));
  if (!dependencies.resolveTestIdentity) return Promise.reject(new Error("test identity resolver is not configured")); return dependencies.resolveTestIdentity(request, context);
}

export function createMiddleware(config: MiddlewareConfig, dependencies: MiddlewareDependencies = {}): Middleware {
  const issues = validateConfig(config); if (issues.length) throw new Error(`invalid middleware config: ${JSON.stringify(issues)}`);
  const operationFailureReporter = dependencies.operationFailureReporter;
  const reportOperationFailure = (failure: OperationFailure): void => {
    void Promise.resolve(operationFailureReporter?.(failure)).catch(() => undefined);
  };
  const observeRejection = (event: RejectionObservation): void => {
    void Promise.resolve(dependencies.rejectionObserver?.(event)).catch(() => undefined);
  };
  return async (request, next) => {
    const started = Date.now(); const requestId = headerToken(request.headers.get(config.settings.requestIdHeader), newId); const incomingTraceId = traceId(request.headers.get(config.settings.traceHeader));
    const context: RequestContext = { requestId, traceId: incomingTraceId, startedAtUnixMs: Date.now(), deadlineUnixMs: Date.now() + config.settings.timeoutMs, baggage: {} };
    const baseOperationContext = operationContextFromRequestContext(context, "http.request");
    return runOperationBoundary(
      baseOperationContext,
      "middleware.http.request",
      () => runWithContext(context, async () => {
        let semanticResponse: Response;
        try {
          semanticResponse = await (async () => {
    if (requestSize(request) > config.settings.maxBodyBytes) return problem(413, "payload_too_large", "request body exceeds configured limit");
    if (!mediaAccepted(request.headers.get("accept"), config.settings.contentRepresentations)) return problem(406, "not_acceptable", "no supported representation requested");
    if (config.settings.tls.requireHttps) {
      const url = new URL(request.url); const forwarded = request.headers.get("x-forwarded-proto"); const trusted = dependencies.isTrustedProxy?.(request) ?? false;
      if (config.settings.tls.strictForwardedHeaders && forwarded && !trusted) return problem(400, "untrusted_forwarded_header", "forwarded transport header from untrusted peer");
      const secure = url.protocol === "https:" || (trusted && forwarded === "https"); if (!secure) return problem(426, "https_required", "HTTPS is required");
    }
    const url = new URL(request.url); const locale = request.headers.get("accept-language"); if (locale) context.locale = locale;
    if (dependencies.authorizeIp && !(await dependencies.authorizeIp(request, context))) return problem(403, "ip_policy_denied", "request source is not permitted");

    const contractFailure = await checkRequestContract(
      dependencies.requestContractValidator,
      request,
      url
    );
    if (contractFailure) {
      const detail = contractFailure.code === "unknown_operation"
        ? "no request contract matched the HTTP method and pathname"
        : "request path, query, headers, or JSON payload failed contract validation";
      return problem(contractFailure.status, contractFailure.code, detail);
    }

    if (config.settings.rateLimit.enabled) {
      const allowed = await dependencies.rateLimiter?.allow(rateKey(config, context, request), config.settings.rateLimit.capacity, config.settings.rateLimit.refillPerSecond); if (allowed === false) return problem(429, "rate_limited", "rate limit exceeded");
    }
    const testDecision = maybeTestIdentity(config, request, dependencies, context);
    let auth: AuthDecision = {};
    try { auth = testDecision ? await testDecision : dependencies.authVerifier ? await dependencies.authVerifier(request, context) : {}; }
    catch { return problem(401, "authentication_failed", "authentication failed"); }
    context.userId = auth.userId; context.tenantId = auth.tenantId; context.baggage = Object.fromEntries(Object.entries(auth.claims || {}).filter(([key]) => key.startsWith("otel.")));
    if (config.integrations.sharedAuth.mode !== "disabled" && !context.userId) return problem(401, "authentication_required", "shared-auth did not establish a user");
    const key = idempotencyKey(config, request); if (key && dependencies.idempotencyStore) { const cached = await dependencies.idempotencyStore.get(key); if (cached && cached.expiresAt > Date.now()) return fromStored(cached); }
    dependencies.telemetry?.started(context, request);
    let response: Response; try { response = await handlerWithTimeout(next, request, config.settings.timeoutMs, request.signal); } catch (error) { response = error instanceof DOMException && error.name === "AbortError" ? problem(499, "request_cancelled", "request was cancelled") : error instanceof Error && error.message === "deadline exceeded" ? problem(504, "deadline_exceeded", "request deadline exceeded") : problem(500, "internal_error", "request handler failed"); }
    response = await maybeCompress(config, request, response); const duration = Date.now() - started;
    try { await dependencies.schemaCapture?.(request, response.clone()); } catch { /* schema capture is observational */ }
    if (dependencies.syncObserver) { try { await dependencies.syncObserver(context, request, response.clone(), duration); } catch { if (!config.integrations.optoSync.failOpen) response = problem(503, "sync_observer_failed", "opto-sync observation failed"); } }
    if (key && dependencies.idempotencyStore && response.ok) await dependencies.idempotencyStore.set(key, await storedResponse(response, config.settings.idempotency.ttlSeconds));
    dependencies.telemetry?.finished(context, request, response, duration); return response;
          })();
        } catch {
          semanticResponse = problem(500, "internal_error", "request processing failed");
        }
        const response = attachHeaders(config, context, semanticResponse);
        if (!response.ok) {
          observeRejection({
            requestId: context.requestId,
            traceId: context.traceId,
            method: request.method,
            pathname: new URL(request.url).pathname,
            status: response.status,
            code: await problemCode(response),
            durationMs: Math.max(0, Date.now() - started)
          });
        }
        return response;
      }),
      {
        mode: "contain",
        errorResult: () => attachHeaders(
          config,
          context,
          problem(500, "internal_error", "request processing failed")
        ),
        reporter: reportOperationFailure,
        attributes: {
          "http.request.method": request.method,
          "url.path": new URL(request.url).pathname
        }
      }
    );
  };
}

async function problemCode(response: Response): Promise<string> {
  try {
    const body = await response.clone().json() as { title?: unknown };
    return typeof body.title === "string" ? body.title : `http_${response.status}`;
  } catch {
    return `http_${response.status}`;
  }
}

function descriptor(): { contractVersion: string; language: string; runtime: string; packageName: string; frameworkAdapters: string[]; capabilities: readonly string[]; operationSymbols: Record<string, string> } {
  return {
    contractVersion,
    language: "typescript",
    runtime: "web-standard-js",
    packageName: "@oresoftware/ores-middleware",
    frameworkAdapters: ["express", "connect", "nestjs", "nextjs", "cloudflare-workers", "bun", "deno"],
    capabilities,
    operationSymbols: {
      descriptor: "descriptor", defaultConfig: "defaultConfig", validateConfig: "validateConfig", createMiddleware: "createMiddleware", runWithContext: "runWithContext", currentContext: "currentContext", capabilities: "capabilities"
    }
  };
}

if (typeof process !== "undefined" && process.argv?.[1]?.endsWith("contractcheck.js")) console.log(JSON.stringify(descriptor()));
