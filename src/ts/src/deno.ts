import { fetchHandler } from "./fetch-adapter.js";
import type { PortableMiddleware } from "./index.js";

/** Structural Deno.serve-compatible handler without taking a dependency on Deno types. */
export type DenoHandler<Info = unknown> = (
  request: Request,
  info: Info
) => Response | Promise<Response>;

/**
 * Adapt the portable middleware core to Deno's Fetch handler shape.
 * Listener ownership, TLS, shutdown, and permissions stay with the consumer.
 */
export function denoHandler<Info = unknown>(
  middleware: PortableMiddleware,
  handler: DenoHandler<Info>
): DenoHandler<Info> {
  return fetchHandler(middleware, handler);
}
