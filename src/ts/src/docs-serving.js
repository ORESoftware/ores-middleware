export const DOCS_FORMAT_HEADER = "X-Ores-Docs-Format";
export const CONTRACT_DIGEST_HEADER = "X-Ores-Contract-SHA256";

const HTML_PATHS = new Set(["/docs/api", "/api/docs", "/api-docs"]);
const PATH_REPRESENTATIONS = new Map([
  ["/api/docs.json", "catalog"],
  ["/api-docs.json", "catalog"],
  ["/openapi.json", "openapi"],
  ["/openrpc.json", "openrpc"],
  ["/connect.json", "connect"],
  ["/hyper-schema.json", "hyper-schema"],
]);
const REPRESENTATIONS = new Set([
  "html",
  "catalog",
  "openapi",
  "openrpc",
  "connect",
  "hyper-schema",
]);
const MEDIA_TYPES = new Map([
  ["text/html", "html"],
  ["application/vnd.ores.api-docs+json", "catalog"],
  ["application/json", "catalog"],
  ["application/vnd.oai.openapi+json", "openapi"],
  ["application/openapi+json", "openapi"],
  ["application/openrpc+json", "openrpc"],
  ["application/vnd.ores.connect+json", "connect"],
  ["application/schema+json", "hyper-schema"],
]);
const CONTENT_TYPES = new Map([
  ["html", "text/html; charset=utf-8"],
  ["catalog", "application/vnd.ores.api-docs+json; charset=utf-8"],
  ["openapi", "application/vnd.oai.openapi+json; charset=utf-8"],
  ["openrpc", "application/openrpc+json; charset=utf-8"],
  ["connect", "application/vnd.ores.connect+json; charset=utf-8"],
  ["hyper-schema", "application/schema+json; charset=utf-8"],
]);
const SHA256_HEX = /^[0-9a-f]{64}$/;

function baseHeaders(contentType) {
  return {
    "Cache-Control": "no-store",
    Pragma: "no-cache",
    "X-Content-Type-Options": "nosniff",
    "Referrer-Policy": "no-referrer",
    "Permissions-Policy": "camera=(), microphone=(), geolocation=()",
    Vary: `Accept, ${DOCS_FORMAT_HEADER}`,
    "Content-Type": contentType,
  };
}

function headersForRepresentation(representation, digest) {
  const headers = baseHeaders(CONTENT_TYPES.get(representation));
  if (representation === "html") {
    headers["X-Frame-Options"] = "DENY";
    headers["Content-Security-Policy"] =
      "default-src 'none'; style-src 'unsafe-inline'; img-src 'none'; " +
      "frame-ancestors 'none'; base-uri 'none'; object-src 'none'; " +
      "form-action 'none'; connect-src 'none'; script-src 'none'";
  }
  if (digest) headers[CONTRACT_DIGEST_HEADER] = digest;
  return headers;
}

function reject(action, status, extra = {}) {
  return {
    action,
    status,
    headOnly: false,
    headers: { ...baseHeaders("application/json; charset=utf-8"), ...extra },
  };
}

function parseAccept(value) {
  if (!value || !value.trim()) return [];
  const ranges = [];
  for (const [index, rawPart] of value.split(",").entries()) {
    const parts = rawPart.split(";").map((part) => part.trim());
    const media = (parts.shift() ?? "").toLowerCase();
    if (!media) continue;
    let quality = 1;
    let valid = true;
    for (const parameter of parts) {
      const [rawName, rawValue = ""] = parameter.split("=", 2);
      if (rawName.trim().toLowerCase() !== "q") continue;
      const parsed = Number(rawValue.trim());
      if (!Number.isFinite(parsed) || parsed < 0 || parsed > 1) {
        valid = false;
        break;
      }
      quality = parsed;
    }
    if (!valid || quality <= 0) continue;
    ranges.push({ media, quality, index });
  }
  ranges.sort((a, b) => b.quality - a.quality || a.index - b.index);
  return ranges;
}

function mediaRepresentation(media) {
  if (media === "*/*") return "html";
  if (media === "application/*") return "catalog";
  return MEDIA_TYPES.get(media);
}

function negotiateGeneric(accept) {
  const ranges = parseAccept(accept);
  if (ranges.length === 0) {
    return accept && accept.trim() ? undefined : "html";
  }
  for (const { media } of ranges) {
    const representation = mediaRepresentation(media);
    if (representation) return representation;
  }
  return undefined;
}

function acceptsRepresentation(accept, representation) {
  if (!accept || !accept.trim()) return true;
  const ranges = parseAccept(accept);
  if (ranges.length === 0) return false;
  for (const { media } of ranges) {
    if (media === "*/*") return true;
    if (representation !== "html" && (media === "application/*" || media === "application/json")) {
      return true;
    }
    if (mediaRepresentation(media) === representation) return true;
  }
  return false;
}

function normalizedFormat(value) {
  if (!value || !value.trim()) return undefined;
  const normalized = value.trim().toLowerCase();
  return REPRESENTATIONS.has(normalized) ? normalized : null;
}

function digestFailure(runtimeDigest, docsDigest) {
  const runtimePresent = Boolean(runtimeDigest && runtimeDigest.trim());
  const docsPresent = Boolean(docsDigest && docsDigest.trim());
  if (runtimePresent && !SHA256_HEX.test(runtimeDigest)) return true;
  if (docsPresent && !SHA256_HEX.test(docsDigest)) return true;
  if (runtimePresent && (!docsPresent || runtimeDigest !== docsDigest)) return true;
  return false;
}

export function decideDocs(request) {
  const path = String(request.path ?? "").split("?", 1)[0];
  const generic = HTML_PATHS.has(path);
  const fixedRepresentation = PATH_REPRESENTATIONS.get(path);
  if (!generic && !fixedRepresentation) {
    return { action: "pass", headOnly: false, headers: {} };
  }

  const method = String(request.method ?? "").toUpperCase();
  if (method !== "GET" && method !== "HEAD") {
    return reject("method-not-allowed", 405, { Allow: "GET, HEAD" });
  }

  const runtimeDigest = request.runtimeContractDigest?.trim();
  const docsDigest = request.docsContractDigest?.trim();
  if (digestFailure(runtimeDigest, docsDigest)) {
    return reject("stopped-for-evaluation", 503);
  }

  const format = normalizedFormat(request.format);
  if (format === null) return reject("not-acceptable", 406);

  let representation;
  if (generic) {
    representation = format ?? negotiateGeneric(request.accept);
  } else {
    representation = fixedRepresentation;
    if (format && format !== representation) {
      return reject("not-acceptable", 406);
    }
  }

  if (!representation || !acceptsRepresentation(request.accept, representation)) {
    return reject("not-acceptable", 406);
  }

  return {
    action: "serve",
    status: 200,
    representation,
    headOnly: method === "HEAD",
    headers: headersForRepresentation(representation, docsDigest),
  };
}
