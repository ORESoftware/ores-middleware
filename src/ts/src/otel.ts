import type { BaseLogger, LogContext, LogFields } from "@oresoftware/next-loggers";
import {
  installLogContextProvider,
  runWithLogContext
} from "@oresoftware/next-loggers/context";

import {
  createMiddleware,
  currentContext,
  type MiddlewareConfig,
  type MiddlewareDependencies,
  type PortableMiddleware,
  type RequestContext
} from "./index.js";

export * from "@oresoftware/next-loggers";
export * from "@oresoftware/next-loggers/context";

const requestLoggers = new WeakMap<Request, BaseLogger>();
let contextProviderInstalled = false;

export interface RequestWithLog extends Request {
  readonly log: BaseLogger;
}

export interface OresOtelMiddlewareDependencies extends MiddlewareDependencies {
  /** File/service logger from ores-otel. A child is derived for every request. */
  logger: BaseLogger;
  /** Optional framework-specific request-child factory. */
  requestLogger?: (
    root: BaseLogger,
    request: Request,
    context: RequestContext
  ) => BaseLogger;
}

/**
 * Installs the ores-otel AsyncLocalStorage provider once for this process.
 * This is a log-context provider only; it does not install or replace a global
 * OpenTelemetry SDK/provider.
 */
export function ensureOresLogContextProvider(): void {
  if (contextProviderInstalled) return;
  installLogContextProvider();
  contextProviderInstalled = true;
}

/** Maps the portable, data-only middleware context into ores-otel fields. */
export function toOresLogContext(context: RequestContext): LogContext {
  const fields: LogFields = {
    "request.id": context.requestId,
    "trace.id": context.traceId,
    "request.started_at_unix_ms": context.startedAtUnixMs
  };
  if (context.userId) fields["user.id"] = context.userId;
  if (context.tenantId) fields["tenant.id"] = context.tenantId;
  if (context.locale) fields["request.locale"] = context.locale;
  if (context.deadlineUnixMs !== undefined) {
    fields["request.deadline_unix_ms"] = context.deadlineUnixMs;
  }
  for (const [key, value] of Object.entries(context.baggage)) {
    // The portable core only admits authenticated `otel.*` claims here.
    if (key.startsWith("otel.")) fields[`baggage.${key}`] = value;
  }

  return {
    traceId: context.traceId,
    traceIds: [context.traceId],
    routineId: context.requestId,
    fields,
    ...(context.userId ? { loggedInUser: { id: context.userId } } : {}),
    tags: ["ores-middleware", "request"]
  };
}

/** Creates the default per-request child while preserving root transports. */
export function createRequestLogger(
  root: BaseLogger,
  context: RequestContext
): BaseLogger {
  const logContext = toOresLogContext(context);
  return root.anew({
    name: root.name ? `${root.name}:request` : "request",
    fields: logContext.fields,
    loggedInUser: logContext.loggedInUser
  });
}

/**
 * Adds `request.log` when the runtime Request is extensible and always records
 * the association in a WeakMap for runtimes that keep Request objects sealed.
 */
export function attachRequestLogger(
  request: Request,
  logger: BaseLogger
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

export function requestLogger(request: Request): BaseLogger | undefined {
  const direct = (request as Request & { log?: unknown }).log;
  return direct && typeof direct === "object"
    ? (direct as BaseLogger)
    : requestLoggers.get(request);
}

export function runWithOresLogContext<T>(
  context: RequestContext,
  operation: () => T
): T {
  ensureOresLogContextProvider();
  return runWithLogContext(toOresLogContext(context), operation);
}

/**
 * Composes the portable middleware with ores-otel. Authentication remains owned
 * by the portable stack; the request child is created only after user/tenant
 * identity has been resolved and is available through both ALS and `req.log`.
 */
export function createOresOtelMiddleware(
  config: MiddlewareConfig,
  dependencies: OresOtelMiddlewareDependencies
): PortableMiddleware {
  ensureOresLogContextProvider();

  const telemetry = dependencies.telemetry;
  const middleware = createMiddleware(config, {
    ...dependencies,
    ...(telemetry
      ? {
          telemetry: {
            started(context, request) {
              return runWithOresLogContext(context, () =>
                telemetry.started(context, request)
              );
            },
            finished(context, request, response, durationMs) {
              return runWithOresLogContext(context, () =>
                telemetry.finished(context, request, response, durationMs)
              );
            }
          }
        }
      : {})
  });

  return (request, next) =>
    middleware(request, async (scopedRequest) => {
      const context = currentContext();
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
        await logger.info("request handler started").addFields(requestFields).send();
        try {
          const response = await next(requestWithLog);
          await logger
            .info("request handler completed")
            .addFields({ ...requestFields, "http.response.status_code": response.status })
            .send();
          return response;
        } catch (error) {
          await logger
            .error("request handler failed", error)
            .addFields(requestFields)
            .send();
          throw error;
        }
      });
    });
}
