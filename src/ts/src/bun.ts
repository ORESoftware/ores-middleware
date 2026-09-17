import { fetchHandler } from "./fetch-adapter.js";
import type { PortableMiddleware } from "./index.js";

/** Structural Bun.serve-compatible handler without taking a dependency on Bun types. */
export type BunHandler<Server = unknown> = (
  request: Request,
  server: Server
) => Response | Promise<Response>;

/**
 * Adapt the portable middleware core to Bun's Fetch handler shape.
 * Server ownership, TLS, websocket upgrade policy, and lifecycle stay with the consumer.
 */
export function bunHandler<Server = unknown>(
  middleware: PortableMiddleware,
  handler: BunHandler<Server>
): BunHandler<Server> {
  return fetchHandler(middleware, handler);
}
