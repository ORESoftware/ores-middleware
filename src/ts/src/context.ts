import { AsyncLocalStorage } from "node:async_hooks";

import type { RequestContext } from "./index.js";

const storage = new AsyncLocalStorage<RequestContext>();

/**
 * Node's AsyncLocalStorage isolates requests, but every child async resource in
 * one request would otherwise share the same mutable object. Store a frozen
 * snapshot so sibling promises cannot overwrite each other's correlation data.
 */
function immutableSnapshot(context: RequestContext): RequestContext {
  const snapshot: RequestContext = {
    ...context,
    baggage: { ...context.baggage }
  };
  Object.freeze(snapshot.baggage);
  return Object.freeze(snapshot) as RequestContext;
}

export function runWithContext<T>(context: RequestContext, operation: () => T): T {
  return storage.run(immutableSnapshot(context), operation);
}

export function currentContext(): RequestContext | undefined {
  return storage.getStore();
}

/** O(1) request ID lookup from the active Node/Bun/Deno async scope. */
export function currentRequestId(): string | undefined {
  return storage.getStore()?.requestId;
}

/** O(1) W3C trace ID lookup from the active async scope. */
export function currentTraceId(): string | undefined {
  return storage.getStore()?.traceId;
}

/** O(1) authenticated user ID lookup from the active async scope. */
export function currentUserId(): string | undefined {
  return storage.getStore()?.userId;
}

/** Explicit naming alias for call sites that use "logged-in user" terminology. */
export const currentLoggedInUserId = currentUserId;

/** O(1) authenticated tenant ID lookup from the active async scope. */
export function currentTenantId(): string | undefined {
  return storage.getStore()?.tenantId;
}
