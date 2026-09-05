import { type LogContext } from "@oresoftware/next-loggers/context";
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
export type OperationFailureReporter = (event: OperationFailureEvent) => void | Promise<void>;
export type OperationOutcome<T> = {
    ok: true;
    value: T;
} | {
    ok: false;
    failure: OperationFailure;
};
export interface OperationBoundaryOptions {
    /** Defaults to a snapshot captured when `runOperationBoundary` is called. */
    context?: CapturedOperationContext;
    /** Defaults to the redacted, fail-open ores-otel reporter. */
    reportFailure?: OperationFailureReporter;
}
/** Captures both the middleware carrier and ores-otel's native log carrier. */
export declare function captureOperationContext(): CapturedOperationContext;
/**
 * Builds a fresh immutable operation carrier from an explicit request context.
 * This is used by framework adapters before policy hooks run and again after
 * authentication enriches the actor/tenant fields. Only allow-listed values
 * become ambient log fields.
 */
export declare function operationContextFromRequestContext(context: RequestContext): CapturedOperationContext;
/**
 * Re-enters both captured carriers. Explicitly absent frames mask unrelated
 * ambient state while the callback runs and are restored afterward.
 */
export declare function runWithCapturedOperationContext<T>(snapshot: CapturedOperationContext, operation: () => T): T;
/**
 * Default reporter: emits only bounded classification and correlation fields.
 * The raw cause is intentionally not serialized; applications may provide a
 * separately audited reporter when redacted stack capture is required.
 */
export declare const reportOresOperationFailure: OperationFailureReporter;
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
export declare function runOperationBoundary<T>(descriptor: OperationDescriptor, operation: () => T | Promise<T>, options?: OperationBoundaryOptions): Promise<OperationOutcome<T>>;
/** Captures context at registration time for event emitters and socket loops. */
export declare function bindOperationBoundary<Arguments extends unknown[], Result>(descriptor: OperationDescriptor, operation: (...arguments_: Arguments) => Result | Promise<Result>, options?: Omit<OperationBoundaryOptions, "context">): (...arguments_: Arguments) => Promise<OperationOutcome<Result>>;
//# sourceMappingURL=operation.d.ts.map