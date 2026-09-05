import defaultLogger from "@oresoftware/next-loggers";
import { installLogContextProvider, runWithLogContext } from "@oresoftware/next-loggers/context";
import { createMiddleware, currentContext } from "./index.js";
export * from "@oresoftware/next-loggers";
export * from "@oresoftware/next-loggers/context";
/** Canonical process/file logger exported by ores-otel. */
export const logger = defaultLogger;
/**
 * Creates an independent ores-otel logger while preserving a single package
 * integration surface for middleware consumers. This explicit wrapper avoids
 * relying on transitive star-export behavior for the dependency's constructor.
 */
export function createLogger(options = {}) {
    return defaultLogger.anew(options);
}
const requestLoggers = new WeakMap();
let contextProviderInstalled = false;
function reportRequestLogFailure(phase, error) {
    try {
        const message = error instanceof Error ? error.message : String(error);
        console.warn("[ores-middleware] request log delivery failed", { phase, message });
    }
    catch {
        // Diagnostics must never replace or delay the request outcome.
    }
}
/**
 * Lifecycle telemetry is intentionally detached from the response path. Even a
 * transport hook that throws, rejects, or reports its own failure cannot prevent
 * the handler from running or replace the handler's original response/error.
 */
function emitRequestLog(event, phase) {
    void event.send().catch((error) => reportRequestLogFailure(phase, error));
}
/**
 * Installs the ores-otel AsyncLocalStorage provider once for this process.
 * This is a log-context provider only; it does not install or replace a global
 * OpenTelemetry SDK/provider.
 */
export function ensureOresLogContextProvider() {
    if (contextProviderInstalled)
        return;
    installLogContextProvider();
    contextProviderInstalled = true;
}
/** Maps the portable, data-only middleware context into ores-otel fields. */
export function toOresLogContext(context) {
    const fields = {
        "request.id": context.requestId,
        "trace.id": context.traceId,
        "request.started_at_unix_ms": context.startedAtUnixMs
    };
    if (context.userId)
        fields["user.id"] = context.userId;
    if (context.tenantId)
        fields["tenant.id"] = context.tenantId;
    if (context.locale)
        fields["request.locale"] = context.locale;
    if (context.deadlineUnixMs !== undefined) {
        fields["request.deadline_unix_ms"] = context.deadlineUnixMs;
    }
    for (const [key, value] of Object.entries(context.baggage)) {
        // The portable core only admits authenticated `otel.*` claims here.
        if (key.startsWith("otel."))
            fields[`baggage.${key}`] = value;
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
export function createRequestLogger(root, context) {
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
export function attachRequestLogger(request, logger) {
    requestLoggers.set(request, logger);
    try {
        Object.defineProperty(request, "log", {
            configurable: true,
            enumerable: false,
            writable: false,
            value: logger
        });
    }
    catch {
        // The WeakMap remains the portable fallback.
    }
    return request;
}
export function requestLogger(request) {
    const direct = request.log;
    return direct && typeof direct === "object"
        ? direct
        : requestLoggers.get(request);
}
export function runWithOresLogContext(context, operation) {
    ensureOresLogContextProvider();
    return runWithLogContext(toOresLogContext(context), operation);
}
/**
 * Composes the portable middleware with ores-otel. Authentication remains owned
 * by the portable stack; the request child is created only after user/tenant
 * identity has been resolved and is available through both ALS and `req.log`.
 */
export function createOresOtelMiddleware(config, dependencies) {
    ensureOresLogContextProvider();
    const telemetry = dependencies.telemetry;
    const middleware = createMiddleware(config, {
        ...dependencies,
        ...(telemetry
            ? {
                telemetry: {
                    started(context, request) {
                        return runWithOresLogContext(context, () => telemetry.started(context, request));
                    },
                    finished(context, request, response, durationMs) {
                        return runWithOresLogContext(context, () => telemetry.finished(context, request, response, durationMs));
                    }
                }
            }
            : {})
    });
    return (request, next) => middleware(request, async (scopedRequest) => {
        const context = currentContext();
        if (!context) {
            throw new Error("ores middleware request context is unavailable");
        }
        const logger = dependencies.requestLogger?.(dependencies.logger, scopedRequest, context) ??
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
            const emitTimeout = () => {
                if (deadlineExceeded)
                    return;
                deadlineExceeded = true;
                emitRequestLog(logger
                    .error("request handler timed out")
                    .addFields({
                    ...requestFields,
                    "http.response.status_code": 504,
                    "request.outcome": "timeout",
                    "request.duration_ms": Math.max(0, Date.now() - handlerStartedAt)
                }), "timeout");
            };
            const deadlineTimer = setTimeout(emitTimeout, timeoutMs);
            emitRequestLog(logger.info("request handler started").addFields(requestFields), "started");
            try {
                const response = await next(requestWithLog);
                const durationMs = Math.max(0, Date.now() - handlerStartedAt);
                if (deadlineExceeded || durationMs >= timeoutMs) {
                    emitTimeout();
                }
                else {
                    emitRequestLog(logger
                        .info("request handler completed")
                        .addFields({
                        ...requestFields,
                        "http.response.status_code": response.status,
                        "request.outcome": "completed",
                        "request.duration_ms": durationMs
                    }), "completed");
                }
                return response;
            }
            catch (error) {
                if (!deadlineExceeded) {
                    emitRequestLog(logger
                        .error("request handler failed", error)
                        .addFields({
                        ...requestFields,
                        "request.outcome": "failed",
                        "request.duration_ms": Math.max(0, Date.now() - handlerStartedAt)
                    }), "failed");
                }
                throw error;
            }
            finally {
                clearTimeout(deadlineTimer);
            }
        });
    });
}
//# sourceMappingURL=otel.js.map