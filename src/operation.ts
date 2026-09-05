import defaultLogger from "@oresoftware/next-loggers";
import {
  captureLogContext,
  runWithLogContext,
  type LogContext
} from "@oresoftware/next-loggers/context";

import {
  captureContext,
  runWithCapturedContext
} from "./context.js";
import type { RequestContext } from "./index.js";

export type OperationTransport = "http" | "tcp" | "websocket";
export type OperationScope = "request" | "connection" | "message" | "callback";
export type OperationFailureKind = "error" | "cancelled" | "deadline_exceeded";

export interface OperationDescriptor {
  transport: OperationTransport;
  scope: OperationScope;
  /** Stable low-cardinality name such as `orders.read` or `chat.message`. */
  name: string;
  /** Optional cooperative cancellation source owned by the protocol adapter. */
  signal?: AbortSignal;
}

export interface CapturedOperationContext {
  requestContext: RequestContext | undefined;
  /** Always present. An empty object represents explicitly captured absence. */
  logContext: LogContext;
}

/** Safe failure metadata. It deliberately excludes the exception message and payload. */
export interface OperationFailure {
  kind: OperationFailureKind;
  code: "operation_failed" | "operation_cancelled" | "operation_deadline_exceeded";
  transport: OperationTransport;
  scope: OperationScope;
  operation: string;
  requestId?: string;
  traceId?: string;
  errorType: string;
}

export interface OperationFailureEvent {
  failure: OperationFailure;
  /** Available only to an explicitly configured reporter; never copied to the public failure. */
  cause: unknown;
}

export type OperationFailureReporter = (
  event: OperationFailureEvent
) => void | Promise<void>;

export type OperationOutcome<T> =
  | { ok: true; value: T }
  | { ok: false; failure: OperationFailure };

export interface OperationBoundaryOptions {
  /** Defaults to a snapshot captured when `runOperationBoundary` is called. */
  context?: CapturedOperationContext;
  /** Defaults to the redacted, fail-open ores-otel reporter. */
  reportFailure?: OperationFailureReporter;
}

const safeToken = /^[A-Za-z0-9_.:-]{1,128}$/;

function safeOperationName(value: string): string {
  return safeToken.test(value) ? value : "operation";
}

function safeErrorType(cause: unknown): string {
  const candidate = cause instanceof Error
    ? cause.name
    : typeof cause;
  return safeToken.test(candidate) ? candidate : "UnknownError";
}

function failureKind(
  cause: unknown,
  signal: AbortSignal | undefined
): OperationFailureKind {
  const errorName = cause instanceof Error ? cause.name : "";
  const reasonName = signal?.reason instanceof Error ? signal.reason.name : "";
  if (errorName === "TimeoutError" || reasonName === "TimeoutError") {
    return "deadline_exceeded";
  }
  if (signal?.aborted || errorName === "AbortError") {
    return "cancelled";
  }
  return "error";
}

function failureCode(kind: OperationFailureKind): OperationFailure["code"] {
  switch (kind) {
    case "deadline_exceeded":
      return "operation_deadline_exceeded";
    case "cancelled":
      return "operation_cancelled";
    case "error":
      return "operation_failed";
  }
}

/** Captures both the middleware carrier and ores-otel's native log carrier. */
export function captureOperationContext(): CapturedOperationContext {
  return {
    requestContext: captureContext(),
    // An empty child frame is intentional: it prevents a callback captured
    // outside a request from inheriting whichever request invokes it later.
    logContext: captureLogContext() ?? {}
  };
}

/**
 * Builds a fresh immutable operation carrier from an explicit request context.
 * This is used by framework adapters before policy hooks run and again after
 * authentication enriches the actor/tenant fields. Only allow-listed values
 * become ambient log fields.
 */
export function operationContextFromRequestContext(
  context: RequestContext
): CapturedOperationContext {
  const fields: Record<string, unknown> = {
    "request.id": context.requestId,
    "trace.id": context.traceId,
    "request.started_at_unix_ms": context.startedAtUnixMs
  };
  if (context.spanId) fields["span.id"] = context.spanId;
  if (context.userId) fields["user.id"] = context.userId;
  if (context.tenantId) fields["tenant.id"] = context.tenantId;
  if (context.locale) fields["request.locale"] = context.locale;
  if (context.deadlineUnixMs !== undefined) {
    fields["request.deadline_unix_ms"] = context.deadlineUnixMs;
  }
  for (const [key, value] of Object.entries(context.baggage)) {
    if (key.startsWith("otel.")) fields[`baggage.${key}`] = value;
  }
  return { requestContext: context, logContext: { fields } };
}

/**
 * Re-enters both captured carriers. Explicitly absent frames mask unrelated
 * ambient state while the callback runs and are restored afterward.
 */
export function runWithCapturedOperationContext<T>(
  snapshot: CapturedOperationContext,
  operation: () => T
): T {
  return runWithLogContext(snapshot.logContext, () =>
    runWithCapturedContext(snapshot.requestContext, operation)
  );
}

/**
 * Default reporter: emits only bounded classification and correlation fields.
 * The raw cause is intentionally not serialized; applications may provide a
 * separately audited reporter when redacted stack capture is required.
 */
export const reportOresOperationFailure: OperationFailureReporter = async ({ failure }) => {
  try {
    await defaultLogger
      .error("operation failed")
      .addFields({
        "operation.name": failure.operation,
        "operation.transport": failure.transport,
        "operation.scope": failure.scope,
        "operation.outcome": failure.kind,
        "error.type": failure.errorType,
        ...(failure.requestId === undefined ? {} : { "request.id": failure.requestId }),
        ...(failure.traceId === undefined ? {} : { "trace.id": failure.traceId })
      })
      .send();
  } catch {
    // Logging/export must never replace an application or protocol outcome.
  }
};

async function reportSafely(
  reporter: OperationFailureReporter,
  event: OperationFailureEvent
): Promise<void> {
  try {
    await reporter(event);
  } catch {
    // User-supplied reporters remain fail-open for the guarded operation.
  }
}

/**
 * Executes one HTTP request, TCP connection/callback, or WebSocket message as
 * an isolated failure domain. Exceptions become typed outcomes; they do not
 * reject the event-loop callback or terminate the listener.
 *
 * Cancellation remains cooperative: the operation must observe the supplied
 * signal. The boundary classifies an already-aborted signal and AbortError /
 * TimeoutError failures but does not pretend JavaScript can force-stop a
 * promise that ignores cancellation.
 */
export async function runOperationBoundary<T>(
  descriptor: OperationDescriptor,
  operation: () => T | Promise<T>,
  options: OperationBoundaryOptions = {}
): Promise<OperationOutcome<T>> {
  const snapshot = options.context ?? captureOperationContext();
  const reporter = options.reportFailure ?? reportOresOperationFailure;
  const operationName = safeOperationName(descriptor.name);

  return runWithCapturedOperationContext(snapshot, async () => {
    try {
      if (descriptor.signal?.aborted) {
        throw descriptor.signal.reason ?? new DOMException("operation cancelled", "AbortError");
      }
      return { ok: true, value: await operation() };
    } catch (cause) {
      const kind = failureKind(cause, descriptor.signal);
      const failure: OperationFailure = {
        kind,
        code: failureCode(kind),
        transport: descriptor.transport,
        scope: descriptor.scope,
        operation: operationName,
        requestId: snapshot.requestContext?.requestId,
        traceId: snapshot.requestContext?.traceId,
        errorType: safeErrorType(cause)
      };
      await reportSafely(reporter, { failure, cause });
      return { ok: false, failure };
    }
  });
}

/** Captures context at registration time for event emitters and socket loops. */
export function bindOperationBoundary<Arguments extends unknown[], Result>(
  descriptor: OperationDescriptor,
  operation: (...arguments_: Arguments) => Result | Promise<Result>,
  options: Omit<OperationBoundaryOptions, "context"> = {}
): (...arguments_: Arguments) => Promise<OperationOutcome<Result>> {
  const context = captureOperationContext();
  return (...arguments_) =>
    runOperationBoundary(
      descriptor,
      () => operation(...arguments_),
      { ...options, context }
    );
}
