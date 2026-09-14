export type RedirectStatus = 302 | 303 | 307 | 308;

export interface RedirectPolicy {
  readonly allowExternalOrigins?: readonly string[];
  readonly defaultStatus?: RedirectStatus;
}

export class RedirectPolicyError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "RedirectPolicyError";
    this.code = code;
  }
}

const CONTROL = /[\u0000-\u001f\u007f]/u;
const REDIRECT_STATUSES = new Set<number>([302, 303, 307, 308]);

function normalizedAllowedOrigins(values: readonly string[] | undefined): ReadonlySet<string> {
  const result = new Set<string>();
  for (const value of values ?? []) {
    let url: URL;
    try {
      url = new URL(value);
    } catch {
      throw new RedirectPolicyError("invalid_redirect_allowlist", "redirect origin allowlist contains an invalid URL");
    }
    if ((url.protocol !== "https:" && url.protocol !== "http:") || url.username || url.password || url.pathname !== "/" || url.search || url.hash) {
      throw new RedirectPolicyError("invalid_redirect_allowlist", "redirect allowlist entries must be bare http(s) origins without credentials, path, query, or fragment");
    }
    result.add(url.origin);
  }
  return result;
}

function assertStatus(status: number): asserts status is RedirectStatus {
  if (!REDIRECT_STATUSES.has(status)) {
    throw new RedirectPolicyError("invalid_redirect_status", "redirect status must be one of 302, 303, 307, or 308");
  }
}

/**
 * Resolve and validate an application redirect. Relative and same-origin
 * destinations are accepted by default; cross-origin redirects require an
 * explicit origin allowlist.
 */
export function resolveRedirect(
  request: Request,
  target: string,
  policy: RedirectPolicy = {},
): Readonly<{ location: string; status: RedirectStatus }> {
  if (!target || CONTROL.test(target)) {
    throw new RedirectPolicyError("invalid_redirect_target", "redirect target is empty or contains control characters");
  }
  if (target.startsWith("//")) {
    throw new RedirectPolicyError("protocol_relative_redirect_forbidden", "protocol-relative redirects are not accepted");
  }

  const current = new URL(request.url);
  let destination: URL;
  try {
    destination = new URL(target, current);
  } catch {
    throw new RedirectPolicyError("invalid_redirect_target", "redirect target is not a valid URL");
  }

  if (destination.protocol !== "https:" && destination.protocol !== "http:") {
    throw new RedirectPolicyError("redirect_scheme_forbidden", "redirect target must use http or https");
  }
  if (destination.username || destination.password) {
    throw new RedirectPolicyError("redirect_credentials_forbidden", "redirect target must not contain embedded credentials");
  }

  if (destination.origin !== current.origin) {
    const allowed = normalizedAllowedOrigins(policy.allowExternalOrigins);
    if (!allowed.has(destination.origin)) {
      throw new RedirectPolicyError("redirect_origin_forbidden", "cross-origin redirect target is not allowlisted");
    }
  }

  const status = policy.defaultStatus ?? 302;
  assertStatus(status);
  return Object.freeze({ location: destination.href, status });
}

export function createRedirectResponse(
  request: Request,
  target: string,
  policy: RedirectPolicy = {},
): Response {
  const { location, status } = resolveRedirect(request, target, policy);
  return new Response(null, {
    status,
    headers: {
      location,
      "cache-control": "no-store",
      "x-content-type-options": "nosniff",
    },
  });
}
