import { AsyncLocalStorage } from "node:async_hooks";
import type { RequestContext } from "./index.js";

const storage = new AsyncLocalStorage<RequestContext>();

function immutableSnapshot(context: RequestContext): RequestContext {
  return Object.freeze({ ...context, baggage: Object.freeze({ ...context.baggage }) });
}

export function runWithContext<T>(context: RequestContext, operation: () => T): T {
  return storage.run(immutableSnapshot(context), operation);
}

export function currentContext(): RequestContext | undefined {
  return storage.getStore();
}

/** Captures a defensive snapshot for callbacks, queues, sockets, and detached tasks. */
export function captureContext(): RequestContext | undefined {
  const context = storage.getStore();
  return context === undefined ? undefined : immutableSnapshot(context);
}

/**
 * Re-enters a captured request scope. An explicitly absent snapshot clears an
 * unrelated caller scope for the duration of the callback and restores it
 * afterward, preventing accidental cross-request inheritance.
 */
export function runWithCapturedContext<T>(
  snapshot: RequestContext | undefined,
  operation: () => T
): T {
  return snapshot === undefined
    ? storage.exit(operation)
    : storage.run(immutableSnapshot(snapshot), operation);
}

/** Captures the active request scope once and re-enters it for every callback. */
export function bindContext<Arguments extends unknown[], Result>(
  operation: (...arguments_: Arguments) => Result
): (...arguments_: Arguments) => Result {
  const snapshot = captureContext();
  return (...arguments_) =>
    runWithCapturedContext(snapshot, () => operation(...arguments_));
}

function currentValue<T>(selector: (context: RequestContext) => T): T | undefined {
  const context = storage.getStore();
  return context === undefined ? undefined : selector(context);
}

export function currentRequestId(): string | undefined {
  return currentValue((context) => context.requestId);
}

export function currentTraceId(): string | undefined {
  return currentValue((context) => context.traceId);
}

export function currentUserId(): string | undefined {
  return currentValue((context) => context.userId);
}

export const currentLoggedInUserId = currentUserId;

export function currentTenantId(): string | undefined {
  return currentValue((context) => context.tenantId);
}
