import "@typespec/json-schema";

using TypeSpec.JsonSchema;

/**
 * Independently authored provider-neutral middleware host ABI.
 *
 * Host/process/provider lifecycle is deliberately outside this contract. Local
 * P2 and edge adapters normalize trusted transport facts into these frames.
 */
namespace Ores.Middleware.Host;

@minLength(64)
@maxLength(64)
@pattern("^[0-9a-f]{64}$")
scalar Sha256 extends string;

@minLength(1)
@maxLength(32)
@pattern("^[A-Z0-9_-]+$")
scalar HttpMethod extends string;

@minLength(1)
@maxLength(8192)
@pattern("^/[^\\r\\n\\u0000]*$")
scalar AbsolutePath extends string;

@minLength(1)
@maxLength(128)
scalar SessionId extends string;

@minValue(100)
@maxValue(599)
scalar HttpStatus extends uint16;

@minValue(0)
scalar NonNegativeUInt64 extends uint64;

@pattern("^[^\\r\\n\\u0000]*$")
scalar HeaderValue extends string;

@TypeSpec.JsonSchema.extension("maxProperties", 256)
@TypeSpec.JsonSchema.extension("propertyNames", #{
  `type`: "string",
  `pattern`: "^[a-z0-9!#$%&'*+.^_`|~-]+$"
})
model CanonicalHeaders is Record<HeaderValue>;

enum MiddlewareHostKind {
  local_process,
  cloudflare_worker,
}

enum MiddlewareHostOutcome {
  completed,
  disconnected,
  timed_out,
  child_failed,
}

@jsonSchema
@TypeSpec.JsonSchema.extension("additionalProperties", false)
model MiddlewareHostDescriptor {
  schema: "ores.middleware.host/v1";
  abiVersion: "1";
  @minLength(1)
  middlewareContractVersion: string;
  hostKind: MiddlewareHostKind;
  configSha256: Sha256;
}

@jsonSchema
@TypeSpec.JsonSchema.extension("additionalProperties", false)
model MiddlewareHostRequest {
  schema: "ores.middleware.host/v1";
  method: HttpMethod;
  path: AbsolutePath;
  headers: CanonicalHeaders;
  trustedRemoteIp?: string | null;
  contentLength?: NonNegativeUInt64 | null;
  trustedTransportSecure: boolean;
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model MiddlewareHostPermit {
  decision: "permit";
  schema: "ores.middleware.host/v1";
  sessionId: SessionId;
  @minLength(1)
  requestId: string;
  @minLength(1)
  traceId: string;
  responseHeaders: CanonicalHeaders;
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model MiddlewareHostReject {
  decision: "reject";
  schema: "ores.middleware.host/v1";
  status: HttpStatus;
  @minLength(1)
  code: string;
  message: string;
  headers: CanonicalHeaders;
}

@jsonSchema
union MiddlewareHostBeginResult {
  permit: MiddlewareHostPermit,
  reject: MiddlewareHostReject,
}

@jsonSchema
@TypeSpec.JsonSchema.extension("additionalProperties", false)
model MiddlewareHostFinishRequest {
  schema: "ores.middleware.host/v1";
  sessionId: SessionId;
  status: HttpStatus;
  responseBytes?: NonNegativeUInt64 | null;
  outcome: MiddlewareHostOutcome;
}

@jsonSchema
@TypeSpec.JsonSchema.extension("additionalProperties", false)
model MiddlewareHostFinishResult {
  schema: "ores.middleware.host/v1";
  outcome: MiddlewareHostOutcome;
  responseHeaders: CanonicalHeaders;
}
