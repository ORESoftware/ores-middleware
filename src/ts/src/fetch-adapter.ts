import type { PortableMiddleware } from "./index.js";

/**
 * Fetch boundary for Node, Bun, Deno, Hono handlers, and Next.js route handlers.
 * Preserve framework arguments (including promised Next.js params) by identity,
 * but dispatch the request supplied by middleware, not the unvalidated original.
 * Runtime context/peer authentication remains the caller's responsibility.
 */
export function fetchHandler<Arguments extends unknown[]>(
  middleware: PortableMiddleware,
  handler: (request: Request, ...args: Arguments) => Response | Promise<Response>
): (request: Request, ...args: Arguments) => Promise<Response> {
  return (request, ...args) => {
    let invoked = false;
    return middleware(request, async (scopedRequest) => {
      if (invoked) throw new TypeError("middleware cannot dispatch a handler twice");
      invoked = true;
      const response = await handler(scopedRequest, ...args);
      if (!(response instanceof Response)) {
        throw new TypeError("Fetch handler must return a Response");
      }
      return response;
    });
  };
}
