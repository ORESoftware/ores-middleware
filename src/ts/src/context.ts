import {
  captureExecutionLogContext,
  getCorrelationId as getAmbientCorrelationId,
  getExecutionLogContext,
  getLoggedInUserId as getAmbientLoggedInUserId,
  getRequestId as getAmbientRequestId,
  getSessionId as getAmbientSessionId,
  getTenantId as getAmbientTenantId,
  isAsyncContextTracked,
  runWithCapturedExecutionLogContext,
  runWithExecutionLogContext,
  setExecutionLoggedInUser,
  updateExecutionLogContext,
  type ExecutionLogContext
} from "@oresoftware/next-loggers/execution-context";

import type { RequestContext } from "./index.js";

/** Additional request identity fields supported by ores.request-context.v1. */
export interface RequestContextExtensions {
  readonly schema?: "ores.request-context.v1";
  loggedInUserId?: string;
  sessionId?: string;
  correlationId?: string;
  parentRequestId?: string;
  operation?: string;
  serviceName?: string;
}

export type OresRequestContext = RequestContext & RequestContextExtensions;

/** Shared symbol used only as an explicit Request-bound fallback. */
export const requestContextSymbol = Symbol.for("ores.request-context.v1");

const requestContexts = new WeakMap<Request, OresRequestContext>();

function cloneContext(context: OresRequestContext): OresRequestContext {
  return {
    ...context,
    baggage: { ...context.baggage }
  };
}

export function toExecutionLogContext(
  context: OresRequestContext
): ExecutionLogContext {
  const loggedInUserId = context.loggedInUserId ?? context.userId;
  return {
    requestId: context.requestId,
    ...(loggedInUserId ? { loggedInUserId, loggedInUser: { id: loggedInUserId } } : {}),
    ...(context.tenantId ? { tenantId: context.tenantId } : {}),
    ...(context.sessionId ? { sessionId: context.sessionId } : {}),
    ...(context.correlationId ? { correlationId: context.correlationId } : {}),
    ...(context.parentRequestId ? { parentRequestId: context.parentRequestId } : {}),
    traceId: context.traceId,
    ...(context.spanId ? { spanId: context.spanId } : {}),
    ...(context.operation ? { operation: context.operation } : {}),
    ...(context.serviceName ? { serviceName: context.serviceName } : {}),
    ...(context.locale ? { locale: context.locale } : {}),
    startedAtUnixMs: context.startedAtUnixMs,
    ...(context.deadlineUnixMs === undefined
      ? {}
      : { deadlineUnixMs: context.deadlineUnixMs }),
    baggage: { ...context.baggage },
    routineId: context.requestId,
    tags: ["ores-middleware", "request"]
  };
}

export function fromExecutionLogContext(
  context: ExecutionLogContext | undefined
): OresRequestContext | undefined {
  if (!context?.requestId) return undefined;
  const loggedInUserId =
    context.loggedInUserId ??
    context.loggedInUser?.id ??
    context.loggedInUser?.ddUserId;
  return {
    schema: "ores.request-context.v1",
    requestId: context.requestId,
    traceId: context.traceId ?? "",
    ...(context.spanId ? { spanId: context.spanId } : {}),
    ...(loggedInUserId
      ? { loggedInUserId: String(loggedInUserId), userId: String(loggedInUserId) }
      : {}),
    ...(context.tenantId ? { tenantId: context.tenantId } : {}),
    ...(context.sessionId ? { sessionId: context.sessionId } : {}),
    ...(context.correlationId ? { correlationId: context.correlationId } : {}),
    ...(context.parentRequestId ? { parentRequestId: context.parentRequestId } : {}),
    ...(context.operation ? { operation: context.operation } : {}),
    ...(context.serviceName ? { serviceName: context.serviceName } : {}),
    ...(context.locale ? { locale: context.locale } : {}),
    startedAtUnixMs: context.startedAtUnixMs ?? 0,
    ...(context.deadlineUnixMs === undefined
      ? {}
      : { deadlineUnixMs: context.deadlineUnixMs }),
    baggage: { ...(context.baggage ?? {}) }
  };
}

/**
 * Scope work through the single AsyncLocalStorage instance owned by ores-otel.
 * In workerd without native ALS this executes explicitly and ambient getters
 * remain empty, preventing cross-request data bleed.
 */
export function runWithContext<T>(
  context: OresRequestContext,
  operation: () => T
): T {
  return runWithExecutionLogContext(toExecutionLogContext(context), operation);
}

export function currentContext(): OresRequestContext | undefined {
  return fromExecutionLogContext(getExecutionLogContext());
}

export function captureRequestContext(): OresRequestContext | undefined {
  return fromExecutionLogContext(captureExecutionLogContext());
}

export function runWithCapturedRequestContext<T>(
  snapshot: OresRequestContext | undefined,
  operation: () => T
): T {
  return runWithCapturedExecutionLogContext(
    snapshot ? toExecutionLogContext(snapshot) : undefined,
    operation
  );
}

/**
 * Attach an immutable snapshot to a Fetch Request for runtimes without native
 * async context tracking. The WeakMap remains the fallback for sealed Request
 * implementations.
 */
export function bindRequestContext(
  request: Request,
  context: OresRequestContext
): Request {
  const snapshot = cloneContext(context);
  requestContexts.set(request, snapshot);
  try {
    Object.defineProperty(request, requestContextSymbol, {
      configurable: false,
      enumerable: false,
      writable: false,
      value: snapshot
    });
  } catch {
    // Sealed Fetch Request objects remain supported through the WeakMap.
  }
  return request;
}

export function contextForRequest(
  request: Request
): OresRequestContext | undefined {
  const direct = (request as Request & {
    [requestContextSymbol]?: OresRequestContext;
  })[requestContextSymbol];
  return direct ? cloneContext(direct) : requestContexts.get(request);
}

export function runWithBoundRequestContext<T>(
  request: Request,
  context: OresRequestContext,
  operation: () => T
): T {
  bindRequestContext(request, context);
  return runWithContext(context, operation);
}

export function setLoggedInUserId(userId: string): boolean {
  return setExecutionLoggedInUser({ id: userId });
}

export function updateRequestContext(
  patch: Partial<OresRequestContext>
): boolean {
  const loggedInUserId = patch.loggedInUserId ?? patch.userId;
  return updateExecutionLogContext({
    ...(patch.requestId ? { requestId: patch.requestId } : {}),
    ...(loggedInUserId
      ? { loggedInUserId, loggedInUser: { id: loggedInUserId } }
      : {}),
    ...(patch.tenantId ? { tenantId: patch.tenantId } : {}),
    ...(patch.sessionId ? { sessionId: patch.sessionId } : {}),
    ...(patch.correlationId ? { correlationId: patch.correlationId } : {}),
    ...(patch.parentRequestId ? { parentRequestId: patch.parentRequestId } : {}),
    ...(patch.traceId ? { traceId: patch.traceId } : {}),
    ...(patch.spanId ? { spanId: patch.spanId } : {}),
    ...(patch.operation ? { operation: patch.operation } : {}),
    ...(patch.serviceName ? { serviceName: patch.serviceName } : {}),
    ...(patch.locale ? { locale: patch.locale } : {}),
    ...(patch.startedAtUnixMs === undefined
      ? {}
      : { startedAtUnixMs: patch.startedAtUnixMs }),
    ...(patch.deadlineUnixMs === undefined
      ? {}
      : { deadlineUnixMs: patch.deadlineUnixMs }),
    ...(patch.baggage ? { baggage: { ...patch.baggage } } : {})
  });
}

export const getRequestId = getAmbientRequestId;
export const getLoggedInUserId = getAmbientLoggedInUserId;
export const getTenantId = getAmbientTenantId;
export const getSessionId = getAmbientSessionId;
export const getCorrelationId = getAmbientCorrelationId;
export { isAsyncContextTracked };
