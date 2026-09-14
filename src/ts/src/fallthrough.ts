import type { IncomingMessage, ServerResponse } from "node:http";

export const unmatchedRouteErrorCode = "ores.route.unmatched" as const;
export const unmatchedRouteProblemType = "urn:ores:error:route-unmatched" as const;
export const unmatchedRouteTitle = "No route matched" as const;
export const unmatchedRouteDetail = "The request target is not handled by this server." as const;

export type FallthroughStatus = 421 | 404;

export interface FallthroughOptions {
  /**
   * 421 is the ORES default at the outermost server/router ownership boundary.
   * Use 404 only when framework/deployment compatibility requires conventional
   * not-found behavior at that boundary. Known-route wrong-method handling is
   * still 405 and belongs to the router, not this final handler.
   */
  status?: FallthroughStatus;
}

export interface UnmatchedRouteProblem {
  type: typeof unmatchedRouteProblemType;
  title: typeof unmatchedRouteTitle;
  status: FallthroughStatus;
  code: typeof unmatchedRouteErrorCode;
  detail: typeof unmatchedRouteDetail;
}

const encoder = new TextEncoder();

export function unmatchedRouteProblem(options: FallthroughOptions = {}): UnmatchedRouteProblem {
  const status = options.status ?? 421;
  return {
    type: unmatchedRouteProblemType,
    title: unmatchedRouteTitle,
    status,
    code: unmatchedRouteErrorCode,
    detail: unmatchedRouteDetail
  };
}

function encodedProblem(options: FallthroughOptions): { readonly status: FallthroughStatus; readonly body: string; readonly byteLength: number } {
  const problem = unmatchedRouteProblem(options);
  const body = JSON.stringify(problem);
  return {
    status: problem.status,
    body,
    byteLength: encoder.encode(body).byteLength
  };
}

const headersFor = (byteLength: number): Headers => new Headers({
  "cache-control": "no-store",
  "content-length": String(byteLength),
  "content-type": "application/problem+json; charset=utf-8",
  "x-content-type-options": "nosniff"
});

/**
 * Fetch/WHATWG Response adapter for the canonical ORES final fall-through.
 * The request target is deliberately ignored so paths, queries, and route
 * inventory cannot leak through this response.
 */
export function createFinalFallthroughResponse(
  request: Pick<Request, "method">,
  options: FallthroughOptions = {}
): Response {
  const encoded = encodedProblem(options);
  return new Response(request.method.toUpperCase() === "HEAD" ? null : encoded.body, {
    status: encoded.status,
    headers: headersFor(encoded.byteLength)
  });
}

export type NodeFinalFallthroughHandler = (request: IncomingMessage, response: ServerResponse) => void;

/**
 * Native Node.js handler. It is also compatible with the final `(req, res)`
 * shape used by Connect/Express-style servers.
 */
export function nodeFinalFallthroughHandler(options: FallthroughOptions = {}): NodeFinalFallthroughHandler {
  const encoded = encodedProblem(options);
  return (request, response) => {
    response.statusCode = encoded.status;
    response.setHeader("cache-control", "no-store");
    response.setHeader("content-length", String(encoded.byteLength));
    response.setHeader("content-type", "application/problem+json; charset=utf-8");
    response.setHeader("x-content-type-options", "nosniff");
    response.end(request.method?.toUpperCase() === "HEAD" ? undefined : encoded.body);
  };
}
