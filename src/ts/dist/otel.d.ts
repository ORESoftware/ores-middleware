import defaultLogger from "@oresoftware/next-loggers";
import { type MiddlewareConfig, type MiddlewareDependencies, type PortableMiddleware, type RequestContext } from "./index.js";
export * from "@oresoftware/next-loggers";
export * from "@oresoftware/next-loggers/context";
/** Instance type of the canonical ores-otel default logger export. */
export type OresLogger = typeof defaultLogger;
export type OresLoggerOptions = Parameters<OresLogger["anew"]>[0];
export type OresLogFields = Record<string, unknown>;
export interface OresLogContext {
    loggedInUser?: OresLogFields & {
        id?: string;
    };
    users?: Array<OresLogFields & {
        id?: string;
    }>;
    fields?: OresLogFields;
    traceId?: string;
    traceIds?: string[];
    routineId?: string;
    tags?: string[];
}
/** Canonical process/file logger exported by ores-otel. */
export declare const logger: OresLogger;
/**
 * Creates an independent ores-otel logger while preserving a single package
 * integration surface for middleware consumers. This explicit wrapper avoids
 * relying on transitive star-export behavior for the dependency's constructor.
 */
export declare function createLogger(options?: OresLoggerOptions): OresLogger;
export interface RequestWithLog extends Request {
    readonly log: OresLogger;
}
export interface OresOtelMiddlewareDependencies extends MiddlewareDependencies {
    /** File/service logger from ores-otel. A child is derived for every request. */
    logger: OresLogger;
    /** Optional framework-specific request-child factory. */
    requestLogger?: (root: OresLogger, request: Request, context: RequestContext) => OresLogger;
}
/**
 * Installs the ores-otel AsyncLocalStorage provider once for this process.
 * This is a log-context provider only; it does not install or replace a global
 * OpenTelemetry SDK/provider.
 */
export declare function ensureOresLogContextProvider(): void;
/** Maps the portable, data-only middleware context into ores-otel fields. */
export declare function toOresLogContext(context: RequestContext): OresLogContext;
/** Creates the default per-request child while preserving root transports. */
export declare function createRequestLogger(root: OresLogger, context: RequestContext): OresLogger;
/**
 * Adds `request.log` when the runtime Request is extensible and always records
 * the association in a WeakMap for runtimes that keep Request objects sealed.
 */
export declare function attachRequestLogger(request: Request, logger: OresLogger): RequestWithLog;
export declare function requestLogger(request: Request): OresLogger | undefined;
export declare function runWithOresLogContext<T>(context: RequestContext, operation: () => T): T;
/**
 * Composes the portable middleware with ores-otel. Authentication remains owned
 * by the portable stack; the request child is created only after user/tenant
 * identity has been resolved and is available through both ALS and `req.log`.
 */
export declare function createOresOtelMiddleware(config: MiddlewareConfig, dependencies: OresOtelMiddlewareDependencies): PortableMiddleware;
//# sourceMappingURL=otel.d.ts.map