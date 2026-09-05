import { currentContext } from "./index.js";
export function denoHandler(middleware, handler) {
    return (request) => middleware(request, handler);
}
export const bunHandler = denoHandler;
export const nextjsMiddleware = denoHandler;
export const nodeWebHandler = denoHandler;
/** Stable carrier keys shared by Express, NestJS, and plain node:http adapters. */
export const nodeRequestContextSymbol = Symbol.for("@oresoftware/ores-middleware/request-context");
export const nodeRequestLoggerSymbol = Symbol.for("@oresoftware/ores-middleware/request-logger");
const nodeRequestContexts = new WeakMap();
const nodeRequestLoggers = new WeakMap();
function immutableContextSnapshot(context) {
    const snapshot = {
        ...context,
        baggage: { ...context.baggage }
    };
    Object.freeze(snapshot.baggage);
    return Object.freeze(snapshot);
}
function tryDefine(target, key, value) {
    try {
        if (target[key] !== undefined)
            return;
        Object.defineProperty(target, key, {
            configurable: true,
            enumerable: false,
            writable: false,
            value
        });
    }
    catch {
        // WeakMap storage remains available for sealed framework request objects.
    }
}
/**
 * Copies the active portable context and request child logger onto the native
 * Node request before Express/NestJS dispatches downstream middleware.
 */
function attachNodeRequestScope(nativeRequest, webRequest) {
    const context = currentContext();
    if (context) {
        const snapshot = immutableContextSnapshot(context);
        nodeRequestContexts.set(nativeRequest, snapshot);
        tryDefine(nativeRequest, nodeRequestContextSymbol, snapshot);
        tryDefine(nativeRequest, "oresContext", snapshot);
    }
    const requestLogger = webRequest.log;
    if (requestLogger !== undefined) {
        nodeRequestLoggers.set(nativeRequest, requestLogger);
        tryDefine(nativeRequest, nodeRequestLoggerSymbol, requestLogger);
        tryDefine(nativeRequest, "oresLog", requestLogger);
        // `req.log` is conventional, but never overwrite an application logger.
        tryDefine(nativeRequest, "log", requestLogger);
    }
    return context;
}
/** Fast request-object lookup, including the WeakMap fallback for sealed objects. */
export function nodeRequestContext(request) {
    const carrier = request;
    const direct = carrier[nodeRequestContextSymbol] ?? carrier.oresContext;
    return direct && typeof direct === "object"
        ? direct
        : nodeRequestContexts.get(request);
}
/** Fast request-object lookup of this package's ores-otel request child logger. */
export function nodeRequestLogger(request) {
    const carrier = request;
    const direct = carrier[nodeRequestLoggerSymbol] ?? carrier.oresLog;
    return (direct ?? nodeRequestLoggers.get(request));
}
function nodeHeadersToWeb(headersLike) {
    const headers = new Headers();
    for (const [name, value] of Object.entries(headersLike)) {
        if (Array.isArray(value)) {
            for (const item of value)
                headers.append(name, String(item));
        }
        else if (value !== undefined) {
            headers.set(name, String(value));
        }
    }
    return headers;
}
function nodeRequestToWeb(req) {
    const protocol = req.protocol ?? (req.socket?.encrypted ? "https" : "http");
    const host = req.headers?.host ?? "localhost";
    const url = `${protocol}://${host}${req.originalUrl ?? req.url ?? "/"}`;
    const headers = nodeHeadersToWeb(req.headers ?? {});
    const method = String(req.method ?? "GET").toUpperCase();
    const init = { method, headers };
    if (method !== "GET" && method !== "HEAD") {
        const value = req.rawBody ?? req.body;
        if (value !== undefined) {
            init.body = (typeof value === "string" || value instanceof Uint8Array
                ? value
                : JSON.stringify(value));
        }
    }
    return new Request(url, init);
}
function nodeResponseHeaders(res) {
    const values = typeof res.getHeaders === "function" ? res.getHeaders() : {};
    return nodeHeadersToWeb(values ?? {});
}
function normalizedResponseStatus(value) {
    const status = Number(value);
    return Number.isInteger(status) && status >= 200 && status <= 599 ? status : 200;
}
/** Keep the ALS frame alive until the real Node response completes. */
function observeNodeResponse(res) {
    let resolvePromise;
    let rejectPromise;
    let settled = false;
    const promise = new Promise((resolve, reject) => {
        resolvePromise = resolve;
        rejectPromise = reject;
    });
    const cleanup = () => {
        if (typeof res.removeListener !== "function")
            return;
        res.removeListener("finish", finish);
        res.removeListener("close", finish);
        res.removeListener("error", fail);
    };
    const finish = () => {
        if (settled)
            return;
        settled = true;
        cleanup();
        resolvePromise(new Response(null, {
            status: normalizedResponseStatus(res.statusCode),
            headers: nodeResponseHeaders(res)
        }));
    };
    const fail = (error) => {
        if (settled)
            return;
        settled = true;
        cleanup();
        rejectPromise(error);
    };
    if (res.writableEnded || res.finished) {
        queueMicrotask(finish);
    }
    else if (typeof res.once === "function") {
        res.once("finish", finish);
        res.once("close", finish);
        res.once("error", fail);
    }
    else {
        // Non-standard test doubles cannot expose lifecycle events. Do not hang.
        queueMicrotask(finish);
    }
    return {
        promise,
        cancel() {
            if (settled)
                return;
            settled = true;
            cleanup();
        }
    };
}
function applyEarlyCorrelationHeaders(res, context, options) {
    if (!context || res.headersSent || typeof res.setHeader !== "function")
        return;
    res.setHeader(options.requestIdResponseHeader ?? "x-request-id", context.requestId);
    // A response traceparent belongs to a real active server span. The portable
    // adapter deliberately leaves that header to the runtime tracer.
}
async function writeWebResponse(res, response) {
    if (res.writableEnded || res.finished || res.headersSent)
        return;
    if (typeof res.status === "function")
        res.status(response.status);
    else
        res.statusCode = response.status;
    response.headers.forEach((value, name) => res.setHeader?.(name, value));
    const body = response.body
        ? Buffer.from(await response.arrayBuffer())
        : undefined;
    if (typeof res.end === "function")
        res.end(body);
    else if (typeof res.send === "function")
        res.send(body);
    else
        throw new TypeError("Node response must expose end() or send()");
}
/**
 * Express-compatible middleware that calls `next()` with no callback/error and
 * keeps both middleware and ores-otel ALS frames active until `finish`/`close`.
 */
export function expressMiddleware(middleware, options = {}) {
    return (req, res, next) => {
        void (async () => {
            const request = nodeRequestToWeb(req);
            const response = await middleware(request, async (scopedRequest) => {
                const context = attachNodeRequestScope(req, scopedRequest);
                applyEarlyCorrelationHeaders(res, context, options);
                const observation = observeNodeResponse(res);
                try {
                    next();
                }
                catch (error) {
                    observation.cancel();
                    throw error;
                }
                return observation.promise;
            });
            // Covers middleware short-circuits and deadlines before downstream sends.
            await writeWebResponse(res, response);
        })().catch((error) => next(error));
    };
}
/** NestJS can install this through `app.use(...)` without coupling to RxJS. */
export const nestjsMiddleware = expressMiddleware;
export const createNestjsMiddleware = expressMiddleware;
export function honoMiddleware(middleware) {
    return async (context, next) => {
        const response = await middleware(context.req.raw, async () => {
            await next();
            return context.res;
        });
        context.res = response;
    };
}
export function hapiLifecycle(middleware) {
    return async (request, h) => {
        const url = request.url instanceof URL
            ? request.url
            : new URL(String(request.url), `${request.server.info.protocol}://${request.info.host}`);
        const webRequest = new Request(url, {
            method: request.method.toUpperCase(),
            headers: request.headers,
            body: request.payload ? JSON.stringify(request.payload) : undefined
        });
        const response = await middleware(webRequest, async () => new Response(null, { status: 204 }));
        if (response.status === 204)
            return h.continue;
        return h
            .response(Buffer.from(await response.arrayBuffer()))
            .code(response.status)
            .headers(Object.fromEntries(response.headers.entries()));
    };
}
export function nuxtEventHandler(middleware, handler) {
    return async (event) => {
        const request = event.web?.request ?? event.request;
        if (!(request instanceof Request)) {
            throw new TypeError("Nuxt adapter requires an h3 Web Request bridge");
        }
        return middleware(request, () => handler(event));
    };
}
//# sourceMappingURL=adapters.js.map