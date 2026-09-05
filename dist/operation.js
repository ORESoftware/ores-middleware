import defaultLogger from "@oresoftware/next-loggers";
import { captureLogContext, runWithLogContext } from "@oresoftware/next-loggers/context";
import { captureContext, runWithCapturedContext } from "./context.js";
const safeToken = /^[A-Za-z0-9_.:-]{1,128}$/;
function safeOperationName(value) {
    return safeToken.test(value) ? value : "operation";
}
function safeErrorType(cause) {
    const candidate = cause instanceof Error
        ? cause.name
        : typeof cause;
    return safeToken.test(candidate) ? candidate : "UnknownError";
}
function failureKind(cause, signal) {
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
function failureCode(kind) {
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
export function captureOperationContext() {
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
export function operationContextFromRequestContext(context) {
    const fields = {
        "request.id": context.requestId,
        "trace.id": context.traceId,
        "request.started_at_unix_ms": context.startedAtUnixMs
    };
    if (context.spanId)
        fields["span.id"] = context.spanId;
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
        if (key.startsWith("otel."))
            fields[`baggage.${key}`] = value;
    }
    return { requestContext: context, logContext: { fields } };
}
/**
 * Re-enters both captured carriers. Explicitly absent frames mask unrelated
 * ambient state while the callback runs and are restored afterward.
 */
export function runWithCapturedOperationContext(snapshot, operation) {
    return runWithLogContext(snapshot.logContext, () => runWithCapturedContext(snapshot.requestContext, operation));
}
/**
 * Default reporter: emits only bounded classification and correlation fields.
 * The raw cause is intentionally not serialized; applications may provide a
 * separately audited reporter when redacted stack capture is required.
 */
export const reportOresOperationFailure = async ({ failure }) => {
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
    }
    catch {
        // Logging/export must never replace an application or protocol outcome.
    }
};
async function reportSafely(reporter, event) {
    try {
        await reporter(event);
    }
    catch {
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
export async function runOperationBoundary(descriptor, operation, options = {}) {
    const snapshot = options.context ?? captureOperationContext();
    const reporter = options.reportFailure ?? reportOresOperationFailure;
    const operationName = safeOperationName(descriptor.name);
    return runWithCapturedOperationContext(snapshot, async () => {
        try {
            if (descriptor.signal?.aborted) {
                throw descriptor.signal.reason ?? new DOMException("operation cancelled", "AbortError");
            }
            return { ok: true, value: await operation() };
        }
        catch (cause) {
            const kind = failureKind(cause, descriptor.signal);
            const failure = {
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
export function bindOperationBoundary(descriptor, operation, options = {}) {
    const context = captureOperationContext();
    return (...arguments_) => runOperationBoundary(descriptor, () => operation(...arguments_), { ...options, context });
}
//# sourceMappingURL=operation.js.map