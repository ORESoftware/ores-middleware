import { denoHandler } from "./adapters.js";

/** Modern Next.js network-boundary name. */
export const nextjsProxy = denoHandler;

/** @deprecated Next.js renamed middleware.ts to proxy.ts. */
export const nextjsMiddleware = nextjsProxy;

export * from "./proxy-auth.js";
