/** Peer contract: contracts/idempotency-scope.{tsp,schema.json}. */
export interface IdempotencyScope {
  serviceName: string;
  tenantId: string;
  userId: string;
  method: string;
  path: string;
  query: string;
  idempotencyKey: string;
}

const fields = ["serviceName", "tenantId", "userId", "method", "path", "query", "idempotencyKey"] as const;
const requiredNonempty = new Set<string>(["serviceName", "method", "path", "idempotencyKey"]);

/**
 * Versioned, collision-unambiguous UTF-8 scope framing shared with Go.
 * No legacy-key fallback: old unscoped cache entries must never be replayed.
 * This isolates identities; it does not implement distributed single-flight or
 * detect reuse of a key with a different body within the same identity scope.
 */
export async function scopedIdempotencyKey(scope: IdempotencyScope): Promise<string> {
  if (!scope || Object.keys(scope).length !== fields.length || fields.some(field => !Object.hasOwn(scope, field))) {
    throw new TypeError("idempotency scope must contain exactly the contract fields");
  }
  const encoder = new TextEncoder();
  const chunks = [encoder.encode("ores.middleware.idempotency/v2\0")];
  let length = chunks[0]!.length;
  for (const field of fields) {
    const value = scope[field];
    if (typeof value !== "string" || (requiredNonempty.has(field) && value.length === 0)) {
      throw new TypeError(`invalid idempotency scope field: ${field}`);
    }
    for (const character of value) {
      const code = character.charCodeAt(0);
      if (character.length === 1 && code >= 0xd800 && code <= 0xdfff) {
        throw new TypeError("idempotency scope must contain well-formed Unicode");
      }
    }
    if (field === "path" && !value.startsWith("/")) throw new TypeError("idempotency path must be absolute");
    const bytes = encoder.encode(value);
    if (bytes.length > 0xffffffff) throw new TypeError("idempotency scope field is too large");
    const prefix = new Uint8Array(4);
    new DataView(prefix.buffer).setUint32(0, bytes.length, false);
    chunks.push(prefix, bytes); length += 4 + bytes.length;
  }
  const material = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) { material.set(chunk, offset); offset += chunk.length; }
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", material));
  return "ores:idempotency:v2:" + Array.from(digest, byte => byte.toString(16).padStart(2, "0")).join("");
}
