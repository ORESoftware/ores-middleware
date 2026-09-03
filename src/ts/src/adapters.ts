import {
  contextForRequest,
  currentContext,
  type OresRequestContext
} from "./context.js";
import type { NextHandler, PortableMiddleware } from "./index.js";

export type ContextAwareNextHandler = (
  request: Request,
  context: OresRequestContext | undefined
) => Promise<Response>;

const frameworkContexts = new WeakMap<object, OresRequestContext>();

function cloneContext(context: OresRequestContext): OresRequestContext {
  return { ...context, baggage: { ...context.baggage } };
}

/**
 * Attach the explicit request snapshot to framework-native request objects.
 * `oresRequestContext` is non-enumerable so loggers and serializers do not
 * accidentally duplicate the full context.
 */
export function attachFrameworkRequestContext(
  request: object,
  context: OresRequestContext
): void {
  const snapshot = cloneContext(context);
  frameworkContexts.set(request, snapshot);
  try {
    Object.defineProperties(request, {
      oresRequestContext: {
        configurable: true,
        enumerable: false,
        writable: false,
        value: snapshot
      },
      requestId: {
        configurable: true,
        enumerable: false,
        writable: false,
        value: snapshot.requestId
      },
      loggedInUserId: {
        configurable: true,
        enumerable: false,
        writable: false,
        value: snapshot.loggedInUserId ?? snapshot.userId
      }
    });
  } catch {
    // Sealed framework requests remain supported through the WeakMap.
  }
}

export function frameworkRequestContext(
  request: object
): OresRequestContext | undefined {
  const direct = (request as {
    oresRequestContext?: OresRequestContext;
  }).oresRequestContext;
  if (direct) return cloneContext(direct);
  const stored = frameworkContexts.get(request);
  return stored ? cloneContext(stored) : undefined;
}

function requestContext(request: Request): OresRequestContext | undefined {
  return contextForRequest(request) ?? currentContext();
}

export function denoHandler(
  middleware: PortableMiddleware,
  handler: NextHandler
): NextHandler {
  return (request) => middleware(request, handler);
}

export const bunHandler = denoHandler;
export const nextjsMiddleware = denoHandler;
export const nodeWebHandler = denoHandler;

/** Next.js Node runtime: ALS is ambient and an explicit context is also passed. */
export function nextjsNodeHandler(
  middleware: PortableMiddleware,
  handler: ContextAwareNextHandler
): NextHandler {
  return (request) =>
    middleware(request, (scopedRequest) =>
      handler(scopedRequest, requestContext(scopedRequest))
    );
}

/**
 * Next.js Edge/workerd: callers use the explicit second argument. Ambient
 * getters deliberately stay empty when the runtime lacks native ALS.
 */
export function nextjsEdgeHandler(
  middleware: PortableMiddleware,
  handler: ContextAwareNextHandler
): NextHandler {
  return (request) =>
    middleware(request, (scopedRequest) =>
      handler(scopedRequest, contextForRequest(scopedRequest))
    );
}

export function expressMiddleware(middleware: PortableMiddleware) {
  return async (req: any, res: any, next: (error?: unknown) => void) => {
    try {
      const protocol = req.protocol ?? (req.socket?.encrypted ? "https" : "http");
      const host = req.headers?.host ?? "localhost";
      const url = `${protocol}://${host}${req.originalUrl ?? req.url ?? "/"}`;
      const headers = new Headers();
      for (const [name, value] of Object.entries(req.headers ?? {})) {
        if (Array.isArray(value)) {
          for (const item of value) headers.append(name, String(item));
        } else if (value !== undefined) {
          headers.set(name, String(value));
        }
      }
      const method = String(req.method ?? "GET").toUpperCase();
      const body =
        method === "GET" || method === "HEAD"
          ? undefined
          : req.rawBody ??
            (req.body === undefined
              ? undefined
              : typeof req.body === "string"
                ? req.body
                : JSON.stringify(req.body));
      const request = new Request(url, { method, headers, body });
      const response = await middleware(request, async (scopedRequest) => {
        const context = requestContext(scopedRequest);
        if (context) attachFrameworkRequestContext(req, context);
        await new Promise<void>((resolve, reject) =>
          next((error?: unknown) => (error ? reject(error) : resolve()))
        );
        return new Response(null, { status: res.statusCode ?? 200 });
      });
      res.status(response.status);
      response.headers.forEach((value, name) => res.setHeader(name, value));
      if (response.body) {
        res.send(Buffer.from(await response.arrayBuffer()));
      } else {
        res.end();
      }
    } catch (error) {
      next(error);
    }
  };
}

/** NestJS can install this through `app.use(...)` without coupling core to RxJS. */
export const nestjsMiddleware = expressMiddleware;

export function honoMiddleware(middleware: PortableMiddleware) {
  return async (context: any, next: () => Promise<void>) => {
    const response = await middleware(context.req.raw, async (scopedRequest) => {
      const oresContext = requestContext(scopedRequest);
      if (oresContext) attachFrameworkRequestContext(context, oresContext);
      await next();
      return context.res;
    });
    context.res = response;
  };
}

export function hapiLifecycle(middleware: PortableMiddleware) {
  return async (request: any, h: any) => {
    const url =
      request.url instanceof URL
        ? request.url
        : new URL(
            String(request.url),
            `${request.server.info.protocol}://${request.info.host}`
          );
    const webRequest = new Request(url, {
      method: request.method.toUpperCase(),
      headers: request.headers as HeadersInit,
      body: request.payload ? JSON.stringify(request.payload) : undefined
    });
    const response = await middleware(webRequest, async (scopedRequest) => {
      const oresContext = requestContext(scopedRequest);
      if (oresContext) attachFrameworkRequestContext(request, oresContext);
      return new Response(null, { status: 204 });
    });
    if (response.status === 204) return h.continue;
    return h
      .response(Buffer.from(await response.arrayBuffer()))
      .code(response.status)
      .headers(Object.fromEntries(response.headers.entries()));
  };
}

export function nuxtEventHandler(
  middleware: PortableMiddleware,
  handler: (event: any) => Promise<Response>
): (event: any) => Promise<Response> {
  return async (event) => {
    const request = event.web?.request ?? event.request;
    if (!(request instanceof Request)) {
      throw new TypeError("Nuxt adapter requires an h3 Web Request bridge");
    }
    return middleware(request, (scopedRequest) => {
      const oresContext = requestContext(scopedRequest);
      if (oresContext) attachFrameworkRequestContext(event, oresContext);
      return handler(event);
    });
  };
}
