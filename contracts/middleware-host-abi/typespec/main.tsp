import "@typespec/json-schema";

using TypeSpec.JsonSchema;

/**
 * Independently authored provider-neutral middleware host ABI.
 *
 * Host/process/provider lifecycle is deliberately outside this contract. Local
 * P2 and edge adapters normalize trusted host facts into these frames.
 *
 * The response lifecycle deliberately separates three facts that Fetch-style
 * middleware APIs often collapse: a response head can become available, the
 * transport may later commit that head, and the body/transport reaches a final
 * terminal boundary after that. Hosts must report only observations they can
 * actually make; a Fetch/worker host must not claim transport commitment merely
 * because a Response object exists.
 *
 * Provider names are intentionally not an ABI enum. New edge providers can be
 * added by publishing a new adapter id plus its truthful capability profile.
 */
namespace Ores.Middleware.Host;

@minLength(64)
@maxLength(64)
@pattern("^[0-9a-f]{64}$")
scalar Sha256 extends string;

@minLength(1)
@maxLength(128)
@pattern("^[a-z0-9][a-z0-9._/-]*$")
scalar AdapterId extends string;

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

enum MiddlewareHostExecutionModel {
  local_process,
  fetch_handler,
  event_hooks,
  lambda_events,
  wasi_http,
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model MiddlewareHostCapabilities {
  streamingResponse: boolean;
  responseHeadAvailableObservation: boolean;
  responseHeadCommitObservation: boolean;
  bodyStreamCompletionObservation: boolean;
  transportCompletionObservation: boolean;
  clientDisconnectObservation: boolean;
  backgroundWaitUntil: boolean;
  wasmModule: boolean;
}

enum MiddlewareHostResponseHeadPhase {
  /** Status/headers exist and are available to the host, but need not be on the wire. */
  available,
  /** The adapter observed the downstream transport commit the response head. */
  committed,
}

enum MiddlewareHostCompletionBoundary {
  /** The response body stream reached its terminal state; transport flush is not claimed. */
  body_stream,
  /** The adapter observed the downstream server transport reach its terminal write/disconnect boundary. */
  transport,
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
  adapterId: AdapterId;
  executionModel: MiddlewareHostExecutionModel;
  capabilities: MiddlewareHostCapabilities;
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
  /** Normalized request admitted for downstream P3 execution after portable middleware mutation. */
  request: MiddlewareHostRequest;
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

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model MiddlewareHostRespond {
  decision: "respond";
  schema: "ores.middleware.host/v1";
  status: HttpStatus;
  headers: CanonicalHeaders;
  /** Small edge/P2 short-circuit body. Large/application streaming bodies belong downstream. */
  @maxItems(65536)
  body: uint8[];
}

@jsonSchema
union MiddlewareHostBeginResult {
  permit: MiddlewareHostPermit,
  reject: MiddlewareHostReject,
  respond: MiddlewareHostRespond,
}

@jsonSchema
@TypeSpec.JsonSchema.extension("additionalProperties", false)
model MiddlewareHostResponseHeadRequest {
  schema: "ores.middleware.host/v1";
  sessionId: SessionId;
  status: HttpStatus;
  phase: MiddlewareHostResponseHeadPhase;
}

@jsonSchema
@TypeSpec.JsonSchema.extension("additionalProperties", false)
model MiddlewareHostResponseHeadResult {
  schema: "ores.middleware.host/v1";
  status: HttpStatus;
  phase: MiddlewareHostResponseHeadPhase;
  elapsedMs: NonNegativeUInt64;
}

@jsonSchema
@TypeSpec.JsonSchema.extension("additionalProperties", false)
model MiddlewareHostFinishRequest {
  schema: "ores.middleware.host/v1";
  sessionId: SessionId;
  /** Logical final HTTP status. It does not by itself claim that the status was committed to a client. */
  status: HttpStatus;
  responseBytes?: NonNegativeUInt64 | null;
  outcome: MiddlewareHostOutcome;
  completionBoundary: MiddlewareHostCompletionBoundary;
}

@jsonSchema
@TypeSpec.JsonSchema.extension("additionalProperties", false)
model MiddlewareHostFinishResult {
  schema: "ores.middleware.host/v1";
  outcome: MiddlewareHostOutcome;
  completionBoundary: MiddlewareHostCompletionBoundary;
  status: HttpStatus;
  timeToResponseHeadAvailableMs?: NonNegativeUInt64 | null;
  timeToResponseHeadCommittedMs?: NonNegativeUInt64 | null;
  totalDurationMs: NonNegativeUInt64;
  responseHeaders: CanonicalHeaders;
}
