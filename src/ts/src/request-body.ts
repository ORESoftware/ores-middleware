/** Buffer at most maxBytes before any JSON/schema parser or application handler. */
export class PayloadTooLargeError extends Error {
  constructor() { super("request body exceeds configured limit"); this.name = "PayloadTooLargeError"; }
}

export async function boundRequestBody(request: Request, maxBytes: number, timeoutMs: number): Promise<Request> {
  if (!Number.isSafeInteger(maxBytes) || maxBytes <= 0 || !Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    throw new TypeError("body byte limit and timeout must be positive");
  }
  request.signal.throwIfAborted();
  if (!request.body) return request;
  const reader = request.body.getReader();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let onAbort: (() => void) | undefined;
  let complete = false;
  const interrupted = new Promise<never>((_, reject) => {
    onAbort = () => reject(request.signal.reason ?? new DOMException("request cancelled", "AbortError"));
    request.signal.addEventListener("abort", onAbort, { once: true });
    timer = setTimeout(() => reject(new DOMException("request body deadline exceeded", "TimeoutError")), timeoutMs);
    // Covers an abort between the initial check and listener registration.
    if (request.signal.aborted) onAbort();
  });
  const chunks: Uint8Array[] = [];
  let size = 0;
  try {
    for (;;) {
      const { done, value } = await Promise.race([reader.read(), interrupted]);
      if (done) break;
      if (!(value instanceof Uint8Array)) throw new TypeError("request body must yield bytes");
      if (value.byteLength > maxBytes - size) throw new PayloadTooLargeError();
      size += value.byteLength;
      chunks.push(value.slice());
    }
    const bytes = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
    const headers = new Headers(request.headers);
    headers.delete("transfer-encoding");
    headers.set("content-length", String(size));
    const bounded = new Request(request, { body: bytes, headers });
    // Preserve the explicitly supported child-logger carrier, never internal
    // Request symbols or arbitrary untrusted properties.
    const log = (request as Request & { log?: unknown }).log;
    if (log !== undefined) Object.defineProperty(bounded, "log", { value: log });
    complete = true;
    return bounded;
  } finally {
    if (timer !== undefined) clearTimeout(timer);
    if (onAbort) request.signal.removeEventListener("abort", onAbort);
    // A producer's cancel callback can itself hang. Do not await it and do not
    // tee an unread original body: neither may prevent a terminal response.
    if (!complete) void reader.cancel().catch(() => {});
    reader.releaseLock();
  }
}
