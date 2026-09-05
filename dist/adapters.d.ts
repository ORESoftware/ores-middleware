import { type NextHandler, type PortableMiddleware, type RequestContext } from "./index.js";
export declare function denoHandler(middleware: PortableMiddleware, handler: NextHandler): NextHandler;
export declare const bunHandler: typeof denoHandler;
export declare const nextjsMiddleware: typeof denoHandler;
export declare const nodeWebHandler: typeof denoHandler;
/** Stable carrier keys shared by Express, NestJS, and plain node:http adapters. */
export declare const nodeRequestContextSymbol: unique symbol;
export declare const nodeRequestLoggerSymbol: unique symbol;
export interface NodeRequestWithOresContext {
    readonly oresContext?: RequestContext;
    readonly oresLog?: unknown;
    readonly log?: unknown;
}
export interface ExpressAdapterOptions {
    /** Response header used for the canonical request ID. */
    requestIdResponseHeader?: string;
    /**
     * Reserved for compatibility. Response trace context is owned by the active
     * tracer; this adapter never echoes an inbound parent or invents a span.
     */
    traceparentResponseHeader?: string;
}
type NodeNext = (error?: unknown) => void;
/** Fast request-object lookup, including the WeakMap fallback for sealed objects. */
export declare function nodeRequestContext(request: object): RequestContext | undefined;
/** Fast request-object lookup of this package's ores-otel request child logger. */
export declare function nodeRequestLogger<T = unknown>(request: object): T | undefined;
/**
 * Express-compatible middleware that calls `next()` with no callback/error and
 * keeps both middleware and ores-otel ALS frames active until `finish`/`close`.
 */
export declare function expressMiddleware(middleware: PortableMiddleware, options?: ExpressAdapterOptions): (req: any, res: any, next: NodeNext) => void;
/** NestJS can install this through `app.use(...)` without coupling to RxJS. */
export declare const nestjsMiddleware: typeof expressMiddleware;
export declare const createNestjsMiddleware: typeof expressMiddleware;
export declare function honoMiddleware(middleware: PortableMiddleware): (context: any, next: () => Promise<void>) => Promise<void>;
export declare function hapiLifecycle(middleware: PortableMiddleware): (request: any, h: any) => Promise<any>;
export declare function nuxtEventHandler(middleware: PortableMiddleware, handler: (event: any) => Promise<Response>): (event: any) => Promise<Response>;
export {};
//# sourceMappingURL=adapters.d.ts.map