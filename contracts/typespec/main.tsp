import "@typespec/http";
import "@typespec/json-schema";

using TypeSpec.Http;

@service(#{ title: "ORES Middleware Contract" })
namespace Ores.Middleware;

/** Normative middleware capabilities. Keep in direct parity with JSON Schema. */
enum MiddlewareCapability {
  requestContext: "request-context",
  panicRecovery: "panic-recovery",
  requestId: "request-id",
  traceContext: "trace-context",
  structuredLogging: "structured-logging",
  metricsRed: "metrics-red",
  deadlineTimeout: "deadline-timeout",
  payloadLimit: "payload-limit",
  rateLimit: "rate-limit",
  auth: "auth",
  syncObserver: "sync-observer",
  json: "json",
  headers: "headers",
  compression: "compression",
  tlsPolicy: "tls-policy",
  securityHeaders: "security-headers",
  idempotency: "idempotency",
  ipPolicy: "ip-policy",
  cacheEtag: "cache-etag",
  contentNegotiation: "content-negotiation",
  faultInjection: "fault-injection",
  testAuthBypass: "test-auth-bypass",
  schemaCapture: "schema-capture",
}

/** Semantic SDK operations. Languages map these to idiomatic exported symbols. */
enum SdkOperation {
  descriptor: "descriptor",
  defaultConfig: "defaultConfig",
  validateConfig: "validateConfig",
  createMiddleware: "createMiddleware",
  runWithContext: "runWithContext",
  currentContext: "currentContext",
  capabilities: "capabilities",
}

enum RuntimeEnvironment {
  development: "development",
  test: "test",
  staging: "staging",
  production: "production",
}

enum IntegrationMode {
  disabled: "disabled",
  http: "http",
  embedded: "embedded",
}

enum RateLimitKey {
  ip: "ip",
  user: "user",
  tenant: "tenant",
  route: "route",
}

enum ContentRepresentation {
  json: "application/json",
  problemJson: "application/problem+json",
  messagePack: "application/msgpack",
  xml: "application/xml",
}

model StringMap {
  [key: string]: string;
}

model RequestContext {
  requestId: string;
  traceId: string;
  spanId?: string;
  tenantId?: string;
  userId?: string;
  locale?: string;
  startedAtUnixMs: int64;
  deadlineUnixMs?: int64;
  baggage: StringMap;
}

model RateLimitPolicy {
  enabled: boolean;
  capacity: int32;
  refillPerSecond: float64;
  keyBy: RateLimitKey[];
}

model CompressionPolicy {
  enabled: boolean;
  minimumBytes: int32;
  algorithms: string[];
}

model TlsPolicy {
  mode: "disabled" | "in-process" | "trusted-proxy";
  requireHttps: boolean;
  strictForwardedHeaders: boolean;
  trustedProxyCidrs: string[];
}

model SecurityHeaderPolicy {
  enabled: boolean;
  hstsMaxAgeSeconds: int64;
  contentSecurityPolicy?: string;
  frameOptions: "DENY" | "SAMEORIGIN";
}

model IdempotencyPolicy {
  enabled: boolean;
  headerName: string;
  ttlSeconds: int64;
  requiredMethods: string[];
}

model FaultInjectionPolicy {
  enabled: boolean;
  latencyMs: int32;
  errorRate: float64;
  dropRate: float64;
}

model TestAuthBypassPolicy {
  enabled: boolean;
  headerName: string;
  allowedCidrs: string[];
}

model MiddlewareSettings {
  requestIdHeader: string;
  traceHeader: string;
  timeoutMs: int64;
  maxBodyBytes: int64;
  contextRegistryMaxEntries: int32;
  contextRegistryTtlMs: int64;
  rateLimit: RateLimitPolicy;
  compression: CompressionPolicy;
  tls: TlsPolicy;
  securityHeaders: SecurityHeaderPolicy;
  idempotency: IdempotencyPolicy;
  faultInjection: FaultInjectionPolicy;
  testAuthBypass: TestAuthBypassPolicy;
  contentRepresentations: ContentRepresentation[];
}

model SharedAuthIntegration {
  mode: IntegrationMode;
  issuer?: string;
  audience?: string;
  jwksUri?: url;
  introspectionUrl?: url;
  failOpen: boolean;
}

model OptoSyncIntegration {
  mode: IntegrationMode;
  endpoint?: url;
  outboxTopic?: string;
  failOpen: boolean;
}

model OresOtelIntegration {
  enabled: boolean;
  serviceName: string;
  exporterEndpoint?: url;
  propagators: string[];
}

model MiddlewareIntegrations {
  sharedAuth: SharedAuthIntegration;
  optoSync: OptoSyncIntegration;
  oresOtel: OresOtelIntegration;
}

@jsonSchema
model MiddlewareStackConfig {
  contractVersion: "1.0.0";
  environment: RuntimeEnvironment;
  requiredCapabilities: MiddlewareCapability[];
  settings: MiddlewareSettings;
  integrations: MiddlewareIntegrations;
}

model AdapterDescriptor {
  contractVersion: "1.0.0";
  language: string;
  runtime: string;
  packageName: string;
  frameworkAdapters: string[];
  capabilities: MiddlewareCapability[];
  operationSymbols: StringMap;
}

model ValidationIssue {
  path: string;
  code: string;
  message: string;
}

model ValidationResult {
  valid: boolean;
  issues: ValidationIssue[];
}

@route("/v1")
interface MiddlewareSdk {
  @get @route("/descriptor")
  descriptor(): AdapterDescriptor;

  @post @route("/config/validate")
  validateConfig(@body config: MiddlewareStackConfig): ValidationResult;
}
