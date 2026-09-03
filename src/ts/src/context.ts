import { AsyncLocalStorage } from "node:async_hooks";

import type { RequestContext } from "./index.js";

const storage = new AsyncLocalStorage<RequestContext>();

export function runWithContext<T>(context: RequestContext, operation: () => T): T {
  return storage.run(context, operation);
}

export function currentContext(): RequestContext | undefined {
  return storage.getStore();
}
