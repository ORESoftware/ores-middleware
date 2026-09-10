import {
  currentContext,
  type PortableMiddleware,
  type RequestContext
} from "./index.js";

export interface KoaAdapterOptions {
  requestIdResponseHeader?: string;
}

function nodeHeadersToWeb(headersLike: Record<string, unknown>): Headers {
  const headers = new Headers();
  for (const [name, value] of Object.entries(headersLike)) {
    if (Array.isArray(value)) {
      for (const item of value) headers.append(name, String(item));
    } else if (value !== undefined) {
      headers.set(name, String(value));
    }
  }
  return headers;
}

function bodyToWeb(body: unknown, headers: Headers): BodyInit | undefined {
  if (body === undefined || body === null) return undefined;
  if (typeof body === "string" || body instanceof Uint8Array) return body;
  if (body instanceof ArrayBuffer || body instanceof Blob || body instanceof URLSearchParams) return body;
  if (!headers.has("content-type")) headers.set("content-type", "application/json; charset=utf-8");
  return JSON.stringify(body);
}

function koaRequest(context: any): Request {
  const native = context.req ?? context.request?.req;
  if (!native) throw new TypeError("Koa adapter requires ctx.req or ctx.request.req");

  const protocol = context.protocol ?? (native.socket?.encrypted ? "https" : "http");
  const host = context.host ?? context.request?.host ?? native.headers?.host ?? "localhost";
  const path = context.originalUrl ?? context.url ?? native.url ?? "/";
  const headers = nodeHeadersToWeb(context.request?.headers ?? native.headers ?? {});
  const method = String(context.method ?? native.method ?? "GET").toUpperCase();
  const init: RequestInit = { method, headers };
  if (method !== "GET" && method !== "HEAD") {
    const body = bodyToWeb(context.request?.body ?? native.rawBody ?? native.body, headers);
    if (body !== undefined) init.body = body;
  }
  return new Request(`${protocol}://${host}${path}`, init);
}

function normalizedStatus(value: unknown): number {
  const status = Number(value);
  return Number.isInteger(status) && status >= 200 && status <= 599 ? status : 200;
}

function koaResponse(context: any): Response {
  const status = normalizedStatus(context.status ?? context.response?.status);
  const headers = nodeHeadersToWeb(context.response?.headers ?? {});
  if (status === 204 || status === 205 || status === 304) {
    return new Response(null, { status, headers });
  }
  const body = bodyToWeb(context.body ?? context.response?.body, headers);
  return new Response(body, { status, headers });
}

function snapshot(context: RequestContext): RequestContext {
  const value = { ...context, baggage: { ...context.baggage } };
  Object.freeze(value.baggage);
  return Object.freeze(value) as RequestContext;
}

function attachScope(context: any, request: Request): void {
  context.state ??= {};
  const active = currentContext();
  if (active) context.state.oresContext = snapshot(active);
  const logger = (request as Request & { log?: unknown }).log;
  if (logger !== undefined) context.state.oresLog = logger;
}

async function applyResponse(context: any, response: Response): Promise<void> {
  context.status = response.status;
  response.headers.forEach((value, name) => {
    if (typeof context.set === "function") context.set(name, value);
    else if (context.response?.set) context.response.set(name, value);
  });

  if (response.status === 204 || response.status === 205 || response.status === 304 || !response.body) {
    context.body = null;
    return;
  }
  context.body = Buffer.from(await response.arrayBuffer());
}

/**
 * Koa middleware backed by the same Fetch Request/Response policy engine used
 * by Bun, Deno, Hono and the Node adapters. Install it after a body parser when
 * parsed request bodies are part of request-contract validation.
 */
export function koaMiddleware(
  middleware: PortableMiddleware,
  options: KoaAdapterOptions = {}
) {
  return async (context: any, next: () => Promise<unknown>): Promise<void> => {
    const response = await middleware(koaRequest(context), async (scopedRequest) => {
      attachScope(context, scopedRequest);
      const active = currentContext();
      if (active && typeof context.set === "function") {
        context.set(options.requestIdResponseHeader ?? "x-request-id", active.requestId);
      }
      await next();
      return koaResponse(context);
    });
    await applyResponse(context, response);
  };
}

export const createKoaMiddleware = koaMiddleware;
