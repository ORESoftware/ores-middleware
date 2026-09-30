const peerIps = new WeakMap<Request, string>();

function normalizeIp(value: unknown): string | undefined {
  if (typeof value !== "string") return undefined;
  const candidate = value.trim();
  if (!candidate || candidate.length > 64 || /[\s,]/.test(candidate)) return undefined;

  if (candidate.includes(":")) {
    try {
      // URL parsing gives us a platform-portable IPv6 syntax check.
      new URL(`http://[${candidate}]/`);
      return candidate.toLowerCase();
    } catch {
      return undefined;
    }
  }

  const octets = candidate.split(".");
  if (octets.length !== 4) return undefined;
  if (!octets.every((octet) => /^\d{1,3}$/.test(octet) && Number(octet) <= 255)) {
    return undefined;
  }
  return octets.map((octet) => String(Number(octet))).join(".");
}

/**
 * Attach a socket/transport-derived peer address to a Web Request without
 * putting the value in user-visible headers. Framework adapters call this
 * before the portable middleware runs.
 */
export function attachTrustedPeerIp(request: Request, value: unknown): Request {
  const normalized = normalizeIp(value);
  if (normalized) peerIps.set(request, normalized);
  return request;
}

function forwardedIp(request: Request): string | undefined {
  const cf = normalizeIp(request.headers.get("cf-connecting-ip"));
  if (cf) return cf;

  const forwarded = request.headers.get("x-forwarded-for");
  if (forwarded) {
    const first = normalizeIp(forwarded.split(",", 1)[0]);
    if (first) return first;
  }

  return normalizeIp(request.headers.get("x-real-ip"));
}

/**
 * Resolve the admission-control identity. Forwarded identity is considered
 * only after the embedding runtime has established that the immediate peer is
 * trusted.
 */
export function effectiveClientIp(
  request: Request,
  trustedProxy: boolean,
  resolveDirect?: (request: Request) => string | undefined
): string | undefined {
  const directPeer = normalizeIp(resolveDirect?.(request)) ?? peerIps.get(request);
  return trustedProxy ? forwardedIp(request) ?? directPeer : directPeer;
}
