import defaultLogger from "@oresoftware/next-loggers";
import {
  installExecutionLogContextProvider,
  runWithExecutionLogContext,
  toLoggerLogContext,
  type ExecutionLogContext
} from "@oresoftware/next-loggers/execution-context";

import {
  bindRequestContext,
  contextForRequest,
  currentContext,
  toExecutionLogContext,
  type OresRequestContext
} from "./context.js";
import {
  createMiddleware,
  type MiddlewareConfig,
  type MiddlewareDependencies,
  type PortableMiddleware,
  type RequestContext
} from "./index.js";

export * from "@oresoftware/next-loggers";
export * from "@oresoftware/next-loggers/execution-context";
export * from "./context.js";

/** Instance type of the canonical ores-otel default logger export. */
export type OresLogger = typeof defaultLogger;
export type OresLoggerOptions = Parameters<OresLogger["anew"]>[0];
export type OresLogFields = Record<string, unknown>;
export type OresLogContext = ExecutionLogContext;

/** Canonical process/file logger exported by ores-otel. */
export const logger: OresLogger = defaultLogger;

/**
 * Creates an independent ores-otel logger while preserving a single package
 * integration surface for middleware consumers. This explicit wrapper avoids
 * relying on transitive star-export behavior for the dependency's constructor.
 */
export function createLogger(options: OresLoggerOptions = {}): OresLogger {
  return defaultLogger.anew(options) as OresLogger;
}

const requestLoggers = new WeakMap<Request, OresLogger>();
let contextProviderInstalled = false;

interface SendableLogEvent {
  send(store?: boolean): Promise<void>;
}

function reportRequestLogFailure(phase: string, error: unknown): void {
  try {
    const message = error instanceof Error ? error.message : String(error);
    console.warn("[ores-middleware] request log delivery failed", { phase, message });
  } catch {
    // Diagnostics must never replace or delay the request outcome.
  }
}

/**
 * Lifecycle telemetry is intentionally detached from the response path. Even a
 * transport hook that throws, rejects, or reports its own failure cannot prevent
 * the handler from running or replace the handler's original response/error.
 */
function emitRequestLog(event: SendableLogEvent, phase: string): void {
  void event.send().catch((error: unknown) => reportRequestLogFailure(phase, error));
}

export interface RequestWithLog extends Request {
  readonly log: OresLogger;
}

export interface OresOtelMiddlewareDependencies extends MiddlewareDependencies {
  /** File/service logger from ores-otel. A child is derived for every request. */
  logger: OresLogger;
  /** Optional framework-specific request-child factory. */
  requestLogger?: (
    root: OresLogger,
    request: Request,
    context: RequestContext
  ) => OresLogger;
}

/**
 * Installs the ores-otel execution-context provider once for this process.
 * This is a logger context provider only; it does not install or replace a
 * global OpenTelemetry SDK/provider.
 */
export function ensureOresLogContextProvider(): void {
  if (contextProviderInstalled) return;
  installExecutionLogContextProvider();
  contextProviderInstalled = true;
}

/** Maps the portable middleware request into the canonical execution context. */
export function toOresLogContext(context: RequestContext): OresLogContext {
  const execution = toExecutionLogContext(context as OresRequestContext);
  const projected = toLoggerLogContext(execution);
  return {
    ...execution,
    fields: {
      ...(projected.fields ?? {}),
      "trace.id": context.traceId
    },
    loggedInUser: projected.loggedInUser,
    traceIds: projected.traceIds,
    routineId: projected.routineId,
    tags: projected.tags
  };
}

/** Creates the default per-request child while preserving root transports. */
export function createRequestLogger(
  root: OresLogger,
  context: RequestContext
): OresLogger {
  const logContext = toOresLogContext(context);
  return root.anew({
    name: root.name ? `${root.name}:request` : "request",
    fields: logContext.fields,
    loggedInUser: logContext.loggedInUser
  }) as OresLogger;
}

/**
 * Adds `request.log` when the runtime Request is extensible and always records
 * the association in a WeakMap for runtimes that keep Request objects sealed.
 */
export function attachRequestLogger(
  request: Request,
  logger: OresLogger
): RequestWithLog {
  requestLoggers.set(request, logger);
  try {
    Object.defineProperty(request, "log", {
      configurable: true,
      enumerable: false,
      writable: false,
      value: logger
    });
  } catch {
    // The WeakMap remains the portable fallback.
  }
  return request as RequestWithLog;
}

export function requestLogger(request: Request): OresLogger | undefined {
  const direct = (request as Request & { log?: unknown }).log;
  return direct && typeof direct === "object"
    ? (direct as OresLogger)
    : requestLoggers.get(request);
}

export function runWithOresLogContext<T>(
  context: RequestContext,
  operation: () => T
): T {
  ensureOresLogContextProvider();
  return runWithExecutionLogContext(toOresLogContext(context), operation);
}

/**
 * Composes portable middleware with ores-otel. The telemetry package owns the
 * native carrier; middleware writes one request snapshot into it after auth.
 * The same immutable snapshot is also bound to Fetch Request for workerd/Next
 * Edge runtimes where native async context tracking is unavailable.
 */
export function createOresOtelMiddleware(
  config: MiddlewareConfig,
  dependencies: OresOtelMiddlewareDependencies
): PortableMiddleware {
  ensureOresLogContextProvider();

  const telemetry = dependencies.telemetry;
  const middleware = createMiddleware(config, {
    ...dependencies,
    telemetry: {
      started(context, request) {
        bindRequestContext(request, context);
        if (!telemetry) return;
        return runWithOresLogContext(context, () =>
          telemetry.started(context, request)
        );
      },
      finished(context, request, response, durationMs) {
        bindRequestContext(request, context);
        if (!telemetry) return;
        return runWithOresLogContext(context, () =>
          telemetry.finished(context, request, response, durationMs)
        );
      }
    }
  });

  return (request, next) =>
    middleware(request, async (scopedRequest) => {
      const context = contextForRequest(scopedRequest) ?? currentContext();
      if (!context) {
        throw new Error("ores middleware request context is unavailable");
      }

      const logger =
        dependencies.requestLogger?.(dependencies.logger, scopedRequest, context) ??
        createRequestLogger(dependencies.logger, context);
      const requestWithLog = attachRequestLogger(scopedRequest, logger);
      const url = new URL(scopedRequest.url);
      const requestFields = {
        "http.request.method": scopedRequest.method,
        "url.path": url.pathname
      };

      return runWithOresLogContext(context, async () => {
        const handlerStartedAt = Date.now();
        const timeoutMs = Math.max(1, config.settings.timeoutMs);
        let deadlineExceeded = false;

        const emitTimeout = (): void => {
          if (deadlineExceeded) return;
          deadlineExceeded = true;
          emitRequestLog(
            logger
              .error("request handler timed out")
              .addFields({
                ...requestFields,
                "http.response.status_code": 504,
                "request.outcome": "timeout",
                "request.duration_ms": Math.max(0, Date.now() - handlerStartedAt)
              }),
            "timeout"
          );
        };

        const deadlineTimer = setTimeout(emitTimeout, timeoutMs);
        emitRequestLog(
          logger.info("request handler started").addFields(requestFields),
          "started"
        );

        try {
          const response = await next(requestWithLog);
          const durationMs = Math.max(0, Date.now() - handlerStartedAt);
          if (deadlineExceeded || durationMs >= timeoutMs) {
            emitTimeout();
          } else {
            emitRequestLog(
              logger
                .info("request handler completed")
                .addFields({
                  ...requestFields,
                  "http.response.status_code": response.status,
                  "request.outcome": "completed",
                  "request.duration_ms": durationMs
                }),
              "completed"
            );
          }
          return response;
        } catch (error) {
          if (!deadlineExceeded) {
            emitRequestLog(
              logger
                .error("request handler failed", error)
                .addFields({
                  ...requestFields,
                  "request.outcome": "failed",
                  "request.duration_ms": Math.max(0, Date.now() - handlerStartedAt)
                }),
              "failed"
            );
          }
          throw error;
        } finally {
          clearTimeout(deadlineTimer);
        }
      });
    });
}
