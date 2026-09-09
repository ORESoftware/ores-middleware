import { Buffer } from "node:buffer";
import type { PortableMiddleware } from "./index.js";

/**
 * Bridge Hapi's already-parsed payload, not an unread Node request stream.
 * Configure Hapi payload.maxBytes before parsing. For exact wire bytes (e.g.
 * signature verification) use a separately audited raw-payload integration.
 */
function requestToWeb(request: any): Request {
  const url = request.url instanceof URL
    ? new URL(request.url)
    : new URL(String(request.url), `${request.server.info.protocol}://${request.info.host}`);
  const headers = new Headers();
  for (const [name, value] of Object.entries(request.headers ?? {})) {
    if (Array.isArray(value)) {
      for (const item of value) headers.append(name, String(item));
    } else if (value !== undefined) {
      headers.set(name, String(value));
    }
  }
  const method = String(request.method).toUpperCase();
  const payload = request.payload;
  let body: BodyInit | undefined;
  if (method !== "GET" && method !== "HEAD" && payload !== undefined) {
    // Hapi uses null for an absent payload. Preserve an explicitly framed JSON
    // null, but do not invent a body for a request that actually had none.
    const framedNull = payload === null && (
      Number(headers.get("content-length")) > 0 || headers.has("transfer-encoding")
    );
    if (typeof payload === "string") body = payload;
    else if (payload instanceof Uint8Array) body = Buffer.from(payload) as BodyInit;
    else if (payload !== null || framedNull) {
      if (payload && typeof payload === "object" && typeof payload.pipe === "function") {
        throw new TypeError("Hapi payload streams require a streaming adapter");
      }
      body = JSON.stringify(payload);
      if (body === undefined) throw new TypeError("Hapi payload is not JSON serializable");
    }
  }
  // Hapi has already framed, decoded, and possibly parsed the body. Original
  // framing/compression metadata is not valid for this reconstructed payload.
  headers.delete("content-length");
  headers.delete("transfer-encoding");
  headers.delete("content-encoding");
  return new Request(url, { method, headers, body });
}

async function responseToHapi(response: Response, h: any): Promise<any> {
  if (!(response instanceof Response)) {
    throw new TypeError("Hapi middleware must return a Response");
  }
  const result = h.response(response.body ? Buffer.from(await response.arrayBuffer()) : null)
    .code(response.status);
  for (const [name, value] of response.headers) {
    if (name !== "set-cookie") result.header(name, value);
  }
  // Set-Cookie is not a comma-separated header; Expires itself contains commas.
  for (const cookie of response.headers.getSetCookie()) {
    result.header("set-cookie", cookie, { append: true });
  }
  return result;
}

/**
 * @deprecated Admission-only compatibility hook. It does NOT surround the real
 * route handler, and its synthetic admission response is not completion evidence.
 * Use hapiHandler for deadlines, context, telemetry, and response policies around
 * a Fetch-style route handler. Do not install both for the same route.
 */
export function hapiLifecycle(middleware: PortableMiddleware) {
  return async (request: any, h: any): Promise<any> => {
    let admitted = false;
    const response = await middleware(requestToWeb(request), async () => {
      if (admitted) throw new TypeError("middleware cannot admit a request twice");
      admitted = true;
      return new Response(null, { status: 204 });
    });
    // Status alone is not admission: a real short-circuit 204 must stop routing.
    if (admitted && response.status === 204) return h.continue;
    return (await responseToHapi(response, h)).takeover();
  };
}

/**
 * A real around-handler boundary. The handler must return a Fetch Response, not
 * h.continue/h.abandon or a Hapi response object. This is a buffered HTTP adapter,
 * not an SSE/WebSocket upgrade bridge. Hapi's pre-handler parsing/auth hooks and
 * network response transmission remain outside this handler-level scope.
 */
export function hapiHandler(
  middleware: PortableMiddleware,
  handler: (request: Request, nativeRequest: any, toolkit: any) => Response | Promise<Response>
) {
  return async (request: any, h: any): Promise<any> => {
    let invoked = false;
    const response = await middleware(requestToWeb(request), async (scopedRequest) => {
      if (invoked) throw new TypeError("middleware cannot dispatch a handler twice");
      invoked = true;
      const result = await handler(scopedRequest, request, h);
      if (!(result instanceof Response)) throw new TypeError("Hapi handler must return a Fetch Response");
      return result;
    });
    return responseToHapi(response, h);
  };
}
