import { AsyncLocalStorage } from "node:async_hooks";
const storage = new AsyncLocalStorage();
function immutableSnapshot(context) {
    return Object.freeze({ ...context, baggage: Object.freeze({ ...context.baggage }) });
}
export function runWithContext(context, operation) {
    return storage.run(immutableSnapshot(context), operation);
}
export function currentContext() {
    return storage.getStore();
}
/** Captures a defensive snapshot for callbacks, queues, sockets, and detached tasks. */
export function captureContext() {
    const context = storage.getStore();
    return context === undefined ? undefined : immutableSnapshot(context);
}
/**
 * Re-enters a captured request scope. An explicitly absent snapshot clears an
 * unrelated caller scope for the duration of the callback and restores it
 * afterward, preventing accidental cross-request inheritance.
 */
export function runWithCapturedContext(snapshot, operation) {
    return snapshot === undefined
        ? storage.exit(operation)
        : storage.run(immutableSnapshot(snapshot), operation);
}
/** Captures the active request scope once and re-enters it for every callback. */
export function bindContext(operation) {
    const snapshot = captureContext();
    return (...arguments_) => runWithCapturedContext(snapshot, () => operation(...arguments_));
}
function currentValue(selector) {
    const context = storage.getStore();
    return context === undefined ? undefined : selector(context);
}
export function currentRequestId() {
    return currentValue((context) => context.requestId);
}
export function currentTraceId() {
    return currentValue((context) => context.traceId);
}
export function currentUserId() {
    return currentValue((context) => context.userId);
}
export const currentLoggedInUserId = currentUserId;
export function currentTenantId() {
    return currentValue((context) => context.tenantId);
}
//# sourceMappingURL=context.js.map