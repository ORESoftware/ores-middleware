// Minimal structural declaration: this package intentionally has no dependency
// on @types/node so that browser/edge consumers typecheck without Node types.
declare module 'node:async_hooks' {
  export class AsyncLocalStorage<T> {
    getStore(): T | undefined;
    run<R>(store: T, callback: (...callbackArgs: unknown[]) => R, ...args: unknown[]): R;
    enterWith(store: T): void;
    disable(): void;
  }
}
