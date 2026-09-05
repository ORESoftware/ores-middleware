import type { RequestContext } from "./index.js";
export declare function runWithContext<T>(context: RequestContext, operation: () => T): T;
export declare function currentContext(): RequestContext | undefined;
/** Captures a defensive snapshot for callbacks, queues, sockets, and detached tasks. */
export declare function captureContext(): RequestContext | undefined;
/**
 * Re-enters a captured request scope. An explicitly absent snapshot clears an
 * unrelated caller scope for the duration of the callback and restores it
 * afterward, preventing accidental cross-request inheritance.
 */
export declare function runWithCapturedContext<T>(snapshot: RequestContext | undefined, operation: () => T): T;
/** Captures the active request scope once and re-enters it for every callback. */
export declare function bindContext<Arguments extends unknown[], Result>(operation: (...arguments_: Arguments) => Result): (...arguments_: Arguments) => Result;
export declare function currentRequestId(): string | undefined;
export declare function currentTraceId(): string | undefined;
export declare function currentUserId(): string | undefined;
export declare const currentLoggedInUserId: typeof currentUserId;
export declare function currentTenantId(): string | undefined;
//# sourceMappingURL=context.d.ts.map