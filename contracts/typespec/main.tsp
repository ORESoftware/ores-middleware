import "@typespec/http";
import "@typespec/json-schema";

using TypeSpec.Http;
using TypeSpec.JsonSchema;

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
  ipPrefix: "ip-prefix",
  user: "user",
  subject: "subject",
  email: "email",
  tenant: "tenant",
  organization: "organization",
  session: "session",
  device: "device",
  apiKey: "api-key",
  route: "route",
  method: "method",
}

enum RateLimitAlgorithm {
  tokenBucket: "token-bucket",
  slidingWindowCounter: "sliding-window-counter",
  fixedWindow: "fixed-window",
  concurrency: "concurrency",
}

enum RateLimitLayer {
  cloudflareEdge: "cloudflare-edge",
  kubernetesIngress: "kubernetes-ingress",
  serviceMesh: "service-mesh",
  application: "application",
  authorization: "authorization",
}

enum RateLimitFailureMode {
  failOpen: "fail-open",
  failClosed: "fail-closed",
  localOnly: "local-only",
}

enum RateLimitKeyDerivationMode {
  ephemeralHmacSha256: "ephemeral-hmac-sha256",
  externalHmacSha256: "external-hmac-sha256",
}

enum RateLimitDecisionKind {
  allowed: "allowed",
  denied: "denied",
  degradedAllowed: "degraded-allowed",
  degradedDenied: "degraded-denied",
}

enum RateLimitDecisionSource {
  localMemory: "local-memory",
  redis: "redis",
  cloudflareCache: "cloudflare-cache",
  kubernetesIngress: "kubernetes-ingress",
  serviceMesh: "service-mesh",
  authorization: "authorization",
  unknownSource: "unknown",
}

enum ContentRepresentation {
  html: "text/html",
  json: "application/json",
  problemJson: "application/problem+json",
  messagePack: "application/msgpack",
  xml: "application/xml",
}

model StringMap is Record<string>;

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

/**
 * Layered rate-limit policy. New fields are optional for 1.0 compatibility;
 * adapters apply the documented defaults and must never persist raw identity values.
 */
@TypeSpec.JsonSchema.extension("additionalProperties", false)
@TypeSpec.JsonSchema.extension("allOf", #[
  #{
    `if`: #{
      `type`: "object",
      `properties`: #{
        `layer`: #{
          `const`: "cloudflare-edge"
        }
      },
      `required`: #[
        "layer"
      ]
    },
    `then`: #{
      `type`: "object",
      `properties`: #{
        `keyBy`: #{
          `type`: "array",
          `items`: #{
            `enum`: #[
              "ip",
              "ip-prefix",
              "route",
              "method"
            ]
          },
          `contains`: #{
            `enum`: #[
              "ip",
              "ip-prefix"
            ]
          },
          `minContains`: 1
        }
      }
    }
  },
  #{
    `not`: #{
      `type`: "object",
      `properties`: #{
        `layer`: #{
          `const`: "authorization"
        },
        `failureMode`: #{
          `const`: "fail-open"
        }
      },
      `required`: #[
        "layer",
        "failureMode"
      ]
    }
  }
])
model RateLimitPolicy {
  enabled: boolean;
  @minValue(1)
  capacity: int32;
  @minValueExclusive(0)
  refillPerSecond: float64;
  @minItems(1)
  @TypeSpec.JsonSchema.uniqueItems
  keyBy: RateLimitKey[];
  @minLength(1)
  policyId?: string;
  algorithm?: RateLimitAlgorithm;
  layer?: RateLimitLayer;
  failureMode?: RateLimitFailureMode;
  @minValue(1)
  windowMs?: int64;
  @minValue(1)
  @maxValue(10000)
  localCacheMaxEntries?: int32;
  @minValue(1)
  localCacheTtlMs?: int64;
  @minLength(1)
  keyNamespace?: string;
  @minLength(1)
  keyVersion?: string;
  keyDerivation?: RateLimitKeyDerivationMode;
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model RateLimitPrincipal {
  @pattern("^[0-9a-f]{64}$")
  digest: string;
  @minLength(1)
  keyVersion: string;
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model RateLimitDecision {
  kind: RateLimitDecisionKind;
  source: RateLimitDecisionSource;
  @minLength(1)
  policyId: string;
  layer: RateLimitLayer;
  algorithm: RateLimitAlgorithm;
  @minValue(1)
  limit: int32;
  @minValue(0)
  remaining: int32;
  @minValue(0)
  retryAfterMs?: int64;
  @minValue(0)
  resetAfterMs?: int64;
  @minLength(1)
  reasonCode?: string;
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model CompressionPolicy {
  enabled: boolean;
  @minValue(0)
  minimumBytes: int32;
  @TypeSpec.JsonSchema.uniqueItems
  algorithms: ("br" | "gzip" | "deflate" | "zstd")[];
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model TlsPolicy {
  mode: "disabled" | "in-process" | "trusted-proxy";
  requireHttps: boolean;
  strictForwardedHeaders: boolean;
  trustedProxyCidrs: string[];
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model SecurityHeaderPolicy {
  enabled: boolean;
  @minValue(0)
  hstsMaxAgeSeconds: int64;
  contentSecurityPolicy?: string;
  frameOptions: "DENY" | "SAMEORIGIN";
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model IdempotencyPolicy {
  enabled: boolean;
  @minLength(1)
  headerName: string;
  @minValue(1)
  ttlSeconds: int64;
  @TypeSpec.JsonSchema.uniqueItems
  requiredMethods: ("POST" | "PUT" | "PATCH" | "DELETE")[];
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model FaultInjectionPolicy {
  enabled: boolean;
  @minValue(0)
  latencyMs: int32;
  @minValue(0)
  @maxValue(1)
  errorRate: float64;
  @minValue(0)
  @maxValue(1)
  dropRate: float64;
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model TestAuthBypassPolicy {
  enabled: boolean;
  @minLength(1)
  headerName: string;
  allowedCidrs: string[];
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model MiddlewareSettings {
  @minLength(1)
  requestIdHeader: string;
  @minLength(1)
  traceHeader: string;
  @minValue(1)
  timeoutMs: int64;
  @minValue(1)
  maxBodyBytes: int64;
  @minValue(0)
  contextRegistryMaxEntries: int32;
  @minValue(1)
  contextRegistryTtlMs: int64;
  rateLimit: RateLimitPolicy;
  compression: CompressionPolicy;
  tls: TlsPolicy;
  securityHeaders: SecurityHeaderPolicy;
  idempotency: IdempotencyPolicy;
  faultInjection: FaultInjectionPolicy;
  testAuthBypass: TestAuthBypassPolicy;
  @minItems(1)
  @TypeSpec.JsonSchema.uniqueItems
  contentRepresentations: ContentRepresentation[];
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model SharedAuthIntegration {
  mode: IntegrationMode;
  issuer?: url;
  audience?: string;
  jwksUri?: url;
  introspectionUrl?: url;
  failOpen: false;
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model OptoSyncIntegration {
  mode: IntegrationMode;
  endpoint?: url;
  outboxTopic?: string;
  failOpen: boolean;
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model OresOtelIntegration {
  enabled: boolean;
  @minLength(1)
  serviceName: string;
  exporterEndpoint?: url;
  @minItems(1)
  @TypeSpec.JsonSchema.uniqueItems
  propagators: ("tracecontext" | "baggage" | "b3" | "jaeger")[];
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model MiddlewareIntegrations {
  sharedAuth: SharedAuthIntegration;
  optoSync: OptoSyncIntegration;
  oresOtel: OresOtelIntegration;
}

@jsonSchema
@TypeSpec.JsonSchema.extension("additionalProperties", false)
@TypeSpec.JsonSchema.extension("allOf", #[
  #{
    `if`: #{
      `type`: "object",
      `properties`: #{
        `environment`: #{
          `const`: "production"
        },
        `settings`: #{
          `type`: "object",
          `properties`: #{
            `rateLimit`: #{
              `type`: "object",
              `properties`: #{
                `enabled`: #{
                  `const`: true
                }
              },
              `required`: #[
                "enabled"
              ]
            }
          },
          `required`: #[
            "rateLimit"
          ]
        }
      },
      `required`: #[
        "environment",
        "settings"
      ]
    },
    `then`: #{
      `type`: "object",
      `properties`: #{
        `settings`: #{
          `type`: "object",
          `properties`: #{
            `rateLimit`: #{
              `type`: "object",
              `properties`: #{
                `keyDerivation`: #{
                  `const`: "external-hmac-sha256"
                }
              },
              `required`: #[
                "keyDerivation"
              ]
            }
          }
        }
      }
    }
  }
])
model MiddlewareStackConfig {
  contractVersion: "1.0.0";
  environment: RuntimeEnvironment;
  @minItems(1)
  @TypeSpec.JsonSchema.uniqueItems
  requiredCapabilities: MiddlewareCapability[];
  settings: MiddlewareSettings;
  integrations: MiddlewareIntegrations;
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model AdapterDescriptor {
  contractVersion: "1.0.0";
  language: "rust" | "ts" | "gleam" | "golang" | "elixir" | "erlang";
  @minLength(1)
  runtime: string;
  @minLength(1)
  packageName: string;
  @minItems(1)
  @TypeSpec.JsonSchema.uniqueItems
  frameworkAdapters: string[];
  @minItems(1)
  @TypeSpec.JsonSchema.uniqueItems
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
