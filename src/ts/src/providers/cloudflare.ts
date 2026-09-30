import type { RequestContext } from "../index.js";
import {
  invokeEdgeMinimal,
  type EdgeMinimalCallback,
  type EdgeMinimalDependencies
} from "../edge-provider-api.js";

/**
 * Structural subset of Cloudflare's execution context used by the adapter.
 * Keeping this interface local avoids making Cloudflare runtime types a
 * transitive dependency of portable middleware packages.
 */
export interface CloudflareExecutionContext {
  waitUntil(promise: Promise<unknown>): void;
  passThroughOnException?(): void;
}

export interface CloudflarePortableAdapterOptions<Env> {
  context(
    request: Request,
    env: Env,
    executionContext: CloudflareExecutionContext
  ): RequestContext | Promise<RequestContext>;

  dependencies(
    request: Request,
    env: Env,
    executionContext: CloudflareExecutionContext
  ): EdgeMinimalDependencies | Promise<EdgeMinimalDependencies>;

  /**
   * Provider-owned continuation. Bindings, service bindings, Durable Objects,
   * KV, D1, R2 and other Cloudflare-specific facilities stay on this side of
   * the portable middleware boundary.
   */
  origin(
    request: Request,
    env: Env,
    executionContext: CloudflareExecutionContext
  ): Response | Promise<Response>;
}

export interface CloudflarePortableHandler<Env> {
  fetch(
    request: Request,
    env: Env,
    executionContext: CloudflareExecutionContext
  ): Promise<Response>;
}

/**
 * Adapt one provider-neutral `edge_minimal` middleware callback to the
 * Cloudflare Module Worker fetch shape.
 *
 * Authored middleware never receives `env` or the Cloudflare execution context.
 * The deployment adapter converts provider bindings into the narrow ORES
 * capability interfaces and owns the downstream continuation.
 */
export function createCloudflareEdgeMinimalHandler<Env>(
  middleware: EdgeMinimalCallback,
  options: CloudflarePortableAdapterOptions<Env>
): CloudflarePortableHandler<Env> {
  return {
    async fetch(request, env, executionContext) {
      const context = await options.context(request, env, executionContext);
      const dependencies = await options.dependencies(
        request,
        env,
        executionContext
      );
      const decision = await invokeEdgeMinimal(
        middleware,
        request,
        context,
        dependencies
      );

      if (decision.kind === "respond") {
        return decision.response;
      }

      return await options.origin(decision.request, env, executionContext);
    }
  };
}
