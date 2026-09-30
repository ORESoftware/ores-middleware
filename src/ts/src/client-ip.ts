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


type ParsedIp = { bits: 32 | 128; value: bigint };

function parseIpv4Value(value: string): bigint | undefined {
  const normalized = normalizeIp(value);
  if (!normalized || normalized.includes(":")) return undefined;
  return normalized
    .split(".")
    .reduce((acc, octet) => (acc << 8n) | BigInt(Number(octet)), 0n);
}

function ipv6Words(value: string): number[] | undefined {
  const lower = value.toLowerCase();
  if (lower.split("::").length > 2) return undefined;
  const compressed = lower.includes("::");
  const split = compressed ? lower.split("::") : [lower];
  const headRaw = split[0] ?? "";
  const tailRaw = compressed ? (split[1] ?? "") : undefined;

  const expand = (raw: string | undefined): number[] | undefined => {
    if (!raw) return [];
    const parts = raw.split(":");
    const words: number[] = [];
    for (let index = 0; index < parts.length; index += 1) {
      const part = parts[index];
      if (part === undefined) return undefined;
      if (part.includes(".")) {
        if (index !== parts.length - 1) return undefined;
        const ipv4 = parseIpv4Value(part);
        if (ipv4 === undefined) return undefined;
        words.push(Number((ipv4 >> 16n) & 0xffffn), Number(ipv4 & 0xffffn));
        continue;
      }
      if (!/^[0-9a-f]{1,4}$/.test(part)) return undefined;
      words.push(Number.parseInt(part, 16));
    }
    return words;
  };

  const head = expand(headRaw);
  const tail = expand(tailRaw);
  if (!head || !tail) return undefined;
  if (tailRaw === undefined) return head.length === 8 ? head : undefined;
  const missing = 8 - head.length - tail.length;
  if (missing < 1) return undefined;
  return [...head, ...Array.from({ length: missing }, () => 0), ...tail];
}

function parseIp(value: string): ParsedIp | undefined {
  const normalized = normalizeIp(value);
  if (!normalized) return undefined;
  if (!normalized.includes(":")) {
    const ipv4 = parseIpv4Value(normalized);
    return ipv4 === undefined ? undefined : { bits: 32, value: ipv4 };
  }
  const words = ipv6Words(normalized);
  if (!words || words.length !== 8) return undefined;
  return {
    bits: 128,
    value: words.reduce((acc, word) => (acc << 16n) | BigInt(word), 0n)
  };
}

function ipInCidr(ip: string, cidr: string): boolean {
  const slash = cidr.lastIndexOf("/");
  if (slash <= 0) return false;
  const network = parseIp(cidr.slice(0, slash));
  const candidate = parseIp(ip);
  const prefix = Number(cidr.slice(slash + 1));
  if (
    !network ||
    !candidate ||
    network.bits !== candidate.bits ||
    !Number.isInteger(prefix) ||
    prefix < 0 ||
    prefix > network.bits
  ) {
    return false;
  }
  const shift = BigInt(network.bits - prefix);
  return (network.value >> shift) === (candidate.value >> shift);
}

/** Evaluate only transport-attached peer metadata against configured CIDRs. */
export function attachedPeerIsTrusted(request: Request, cidrs: readonly string[]): boolean {
  const peer = peerIps.get(request);
  return peer !== undefined && cidrs.some((cidr) => ipInCidr(peer, cidr));
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
