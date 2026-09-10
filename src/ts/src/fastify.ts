import {
  currentContext,
  type PortableMiddleware,
  type RequestContext
} from "./index.js";

export interface FastifyAdapterOptions {
  requestIdResponseHeader?: string;
}

type FastifyDone = (error?: unknown) => void;

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

function fastifyRequest(request: any): Request {
  const native = request.raw ?? request;
  const protocol = request.protocol ?? (native.socket?.encrypted ? "https" : "http");
  const host = request.hostname ?? native.headers?.host ?? "localhost";
  const path = request.raw?.url ?? request.url ?? "/";
  const headers = nodeHeadersToWeb(request.headers ?? native.headers ?? {});
  const method = String(request.method ?? native.method ?? "GET").toUpperCase();
  const init: RequestInit = { method, headers };
  if (method !== "GET" && method !== "HEAD") {
    const body = bodyToWeb(request.body ?? native.rawBody ?? native.body, headers);
    if (body !== undefined) init.body = body;
  }
  return new Request(`${protocol}://${host}${path}`, init);
}

function snapshot(context: RequestContext): RequestContext {
  const value = { ...context, baggage: { ...context.baggage } };
  Object.freeze(value.baggage);
  return Object.freeze(value) as RequestContext;
}

function defineIfMissing(target: object, key: PropertyKey, value: unknown): void {
  try {
    if ((target as Record<PropertyKey, unknown>)[key] !== undefined) return;
    Object.defineProperty(target, key, {
      configurable: true,
      enumerable: false,
      writable: false,
      value
    });
  } catch {
    // Framework request wrappers can be sealed; the primary Fastify request
    // object remains the preferred carrier in that case.
  }
}

function attachScope(request: any, scopedRequest: Request): void {
  const active = currentContext();
  if (active) {
    const value = snapshot(active);
    defineIfMissing(request, "oresContext", value);
    if (request.raw && request.raw !== request) defineIfMissing(request.raw, "oresContext", value);
  }
  const logger = (scopedRequest as Request & { log?: unknown }).log;
  if (logger !== undefined) {
    defineIfMissing(request, "oresLog", logger);
    if (request.raw && request.raw !== request) defineIfMissing(request.raw, "oresLog", logger);
  }
}

function normalizedStatus(value: unknown): number {
  const status = Number(value);
  return Number.isInteger(status) && status >= 200 && status <= 599 ? status : 200;
}

function responseFromReply(reply: any): Promise<Response> {
  const raw = reply.raw ?? reply;
  return new Promise<Response>((resolve, reject) => {
    let settled = false;
    const cleanup = (): void => {
      raw.removeListener?.("finish", finish);
      raw.removeListener?.("close", finish);
      raw.removeListener?.("error", fail);
    };
    const finish = (): void => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve(new Response(null, {
        status: normalizedStatus(reply.statusCode ?? raw.statusCode),
        headers: nodeHeadersToWeb(typeof raw.getHeaders === "function" ? raw.getHeaders() : {})
      }));
    };
    const fail = (error: unknown): void => {
      if (settled) return;
      settled = true;
      cleanup();
      reject(error);
    };
    if (raw.writableEnded || raw.finished) queueMicrotask(finish);
    else if (typeof raw.once === "function") {
      raw.once("finish", finish);
      raw.once("close", finish);
      raw.once("error", fail);
    } else queueMicrotask(finish);
  });
}

async function sendPortableResponse(reply: any, response: Response): Promise<void> {
  const raw = reply.raw ?? reply;
  if (reply.sent || raw.writableEnded || raw.finished) return;
  reply.code?.(response.status);
  response.headers.forEach((value, name) => reply.header?.(name, value));
  const body = response.body ? Buffer.from(await response.arrayBuffer()) : undefined;
  if (typeof reply.send === "function") reply.send(body);
  else {
    raw.statusCode = response.status;
    response.headers.forEach((value, name) => raw.setHeader?.(name, value));
    raw.end?.(body);
  }
}

/**
 * Fastify callback-style preHandler hook. It uses Fastify's parsed request body
 * but holds the portable middleware lifecycle open until the native response
 * finishes, so telemetry/deadline finalization observes the real downstream
 * handler rather than only the hook invocation.
 */
export function fastifyPreHandler(
  middleware: PortableMiddleware,
  options: FastifyAdapterOptions = {}
) {
  return (request: any, reply: any, done: FastifyDone): void => {
    let continued = false;
    void (async () => {
      const response = await middleware(fastifyRequest(request), async (scopedRequest) => {
        attachScope(request, scopedRequest);
        const active = currentContext();
        if (active) reply.header?.(options.requestIdResponseHeader ?? "x-request-id", active.requestId);
        continued = true;
        done();
        return responseFromReply(reply);
      });
      if (!continued) await sendPortableResponse(reply, response);
    })().catch((error: unknown) => {
      if (!continued) done(error);
      else request.log?.error?.({ err: error }, "ores-middleware finalization failed after Fastify continuation");
    });
  };
}

export const fastifyMiddleware = fastifyPreHandler;
export const createFastifyMiddleware = fastifyPreHandler;
