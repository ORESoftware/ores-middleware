/**
 * Provider-neutral authentication policy for framework proxy boundaries.
 *
 * Supabase and Neon Auth adapters refresh their own browser sessions and return
 * only normalized evidence plus opaque Set-Cookie values. shared-auth remains
 * the canonical identity authority and must bind both pieces of evidence before
 * any identity header is forwarded to an application handler.
 */

export const pairedAuthProviders = Object.freeze(["supabase", "neon-auth"] as const);
export type PairedAuthProvider = (typeof pairedAuthProviders)[number];

export const proxyIdentityHeaders = Object.freeze({
  authority: "x-ores-auth-authority",
  userId: "x-ores-auth-user-id",
  tenantId: "x-ores-auth-tenant-id",
  evidence: "x-ores-auth-evidence"
} as const);

const CALLER_IDENTITY_HEADER_PREFIXES = Object.freeze([
  "x-ores-auth-",
  "x-clerk-",
  "x-supabase-auth-",
  "x-neon-auth-"
] as const);

const CALLER_IDENTITY_HEADERS = new Set([
  "x-user-id",
  "x-tenant-id",
  "x-auth-user",
  "x-auth-tenant",
  "x-authenticated-user",
  "x-authenticated-tenant"
]);

const MAX_PATH_BYTES = 2_048;
const MAX_IDENTIFIER_BYTES = 512;
const MAX_COOKIE_HEADERS = 16;
const MAX_COOKIE_HEADER_BYTES = 8_192;
const MAX_COOKIE_TOTAL_BYTES = 64 * 1_024;

export type ProxyRouteAccess =
  | "ignore"
  | "public"
  | "anonymous-only"
  | "authenticated-page"
  | "authenticated-api";

export type DynamicProxyRouteAccess = Exclude<ProxyRouteAccess, "ignore">;
export type ProxyRouteMatch = "exact" | "prefix";

export interface ProxyRouteRule {
  readonly path: string;
  readonly match: ProxyRouteMatch;
  readonly access: ProxyRouteAccess;
}

export interface ProxyAuthPolicy {
  readonly routes: readonly ProxyRouteRule[];
  readonly defaultAccess: DynamicProxyRouteAccess;
  readonly signInPath: string;
  readonly signedInPath: string;
  readonly returnToParameter?: string;
}

export interface ProviderSessionIdentity {
  /** Provider-local subject. It is evidence, never the ORES canonical user ID. */
  readonly subject: string;
  readonly tenantId?: string;
  readonly expiresAtUnixMs?: number;
}

export interface ProviderSessionSnapshot {
  readonly provider: PairedAuthProvider;
  /** Absence means this provider observed an anonymous browser session. */
  readonly session?: ProviderSessionIdentity;
  /** Opaque refresh mutations to copy to the eventual response. */
  readonly setCookieHeaders?: readonly string[];
}

export interface PairedProviderSessions {
  readonly supabase: ProviderSessionSnapshot;
  readonly neonAuth: ProviderSessionSnapshot;
}

export type SharedAuthProxyDecision =
  | { readonly kind: "anonymous" }
  | {
      readonly kind: "authenticated";
      readonly userId: string;
      readonly tenantId?: string;
      /** Exact provider subjects accepted and mapped by shared-auth. */
      readonly bindings: Readonly<Record<PairedAuthProvider, string>>;
    }
  | {
      readonly kind: "denied";
      readonly status?: 401 | 403;
      readonly code?: string;
    };

export interface PairedAuthProxyDependencies {
  resolveSupabaseSession(request: Request): Promise<ProviderSessionSnapshot>;
  resolveNeonAuthSession(request: Request): Promise<ProviderSessionSnapshot>;
  /**
   * Canonical authority. Implementations normally call github.com/shared-auth
   * or an embedded shared-auth verifier; provider SDK results alone are never
   * sufficient to establish the application identity.
   */
  verifyWithSharedAuth(
    request: Request,
    sessions: PairedProviderSessions
  ): Promise<SharedAuthProxyDecision>;
}

export interface PairedAuthProxyOptions {
  readonly policy: ProxyAuthPolicy;
  readonly dependencies: PairedAuthProxyDependencies;
}

export interface CanonicalProxyIdentity {
  readonly authority: "shared-auth";
  readonly userId: string;
  readonly tenantId?: string;
  readonly providers: readonly ["supabase", "neon-auth"];
}

export type ProxyAuthResponseCode =
  | "authentication_required"
  | "already_authenticated"
  | "auth_pair_incomplete"
  | "auth_provider_unavailable"
  | "auth_provider_contract_violation"
  | "shared_auth_unavailable"
  | "shared_auth_contract_violation"
  | "shared_auth_rejected"
  | "shared_auth_denied"
  | "auth_evidence_mismatch";

export interface ProxyNextResult {
  readonly kind: "next";
  readonly access: ProxyRouteAccess;
  /** Sanitized request with canonical identity headers only when authenticated. */
  readonly request: Request;
  readonly identity?: CanonicalProxyIdentity;
  /** Copy these values to the framework response without parsing or logging. */
  readonly setCookieHeaders: readonly string[];
}

export interface ProxyResponseResult {
  readonly kind: "response";
  readonly access: DynamicProxyRouteAccess;
  readonly code: ProxyAuthResponseCode;
  readonly response: Response;
}

export type ProxyAuthResult = ProxyNextResult | ProxyResponseResult;
export type PairedAuthProxy = (request: Request) => Promise<ProxyAuthResult>;

export class ProxyAuthConfigError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ProxyAuthConfigError";
  }
}

class ProviderContractError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ProviderContractError";
  }
}

class SharedAuthContractError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "SharedAuthContractError";
  }
}

interface NormalizedProxyPolicy extends ProxyAuthPolicy {
  readonly returnToParameter: string;
}

/** Secure defaults: static framework assets are ignored and every app route is protected. */
export function defaultPairedAuthProxyPolicy(): ProxyAuthPolicy {
  return {
    routes: [
      { path: "/_next", match: "prefix", access: "ignore" },
      { path: "/favicon.ico", match: "exact", access: "ignore" },
      { path: "/robots.txt", match: "exact", access: "ignore" },
      { path: "/sitemap.xml", match: "exact", access: "ignore" },
      { path: "/healthz", match: "exact", access: "ignore" },
      { path: "/readyz", match: "exact", access: "ignore" },
      { path: "/api/auth", match: "prefix", access: "ignore" },
      { path: "/sign-in", match: "prefix", access: "anonymous-only" },
      { path: "/sign-up", match: "prefix", access: "anonymous-only" }
    ],
    defaultAccess: "authenticated-page",
    signInPath: "/sign-in",
    signedInPath: "/app",
    returnToParameter: "returnTo"
  };
}

export function createPairedAuthProxy(options: PairedAuthProxyOptions): PairedAuthProxy {
  if (!options || typeof options !== "object") {
    throw new ProxyAuthConfigError("paired auth proxy options are required");
  }
  if (!options.dependencies || typeof options.dependencies !== "object") {
    throw new ProxyAuthConfigError("paired auth proxy dependencies are required");
  }
  for (const name of [
    "resolveSupabaseSession",
    "resolveNeonAuthSession",
    "verifyWithSharedAuth"
  ] as const) {
    if (typeof options.dependencies[name] !== "function") {
      throw new ProxyAuthConfigError(`dependencies.${name} must be a function`);
    }
  }

  const policy = normalizePolicy(options.policy);
  const dependencies = options.dependencies;

  return async (request: Request): Promise<ProxyAuthResult> => {
    if (!(request instanceof Request)) {
      throw new TypeError("paired auth proxy requires a Web Request");
    }

    // Strip every caller-controlled identity carrier before any provider or
    // application code observes the request. Cookie and Authorization remain
    // available to the provider/session and shared-auth ports.
    const sanitizedRequest = stripCallerIdentityHeaders(request);
    const access = classifyRoute(policy, new URL(sanitizedRequest.url).pathname);
    if (access === "ignore") {
      return nextResult(access, sanitizedRequest, [], undefined);
    }

    const providerResults = await Promise.allSettled([
      dependencies
        .resolveSupabaseSession(sanitizedRequest)
        .then((value) => normalizeProviderSnapshot("supabase", value)),
      dependencies
        .resolveNeonAuthSession(sanitizedRequest)
        .then((value) => normalizeProviderSnapshot("neon-auth", value))
    ]);

    const fulfilled = providerResults
      .filter((result): result is PromiseFulfilledResult<ProviderSessionSnapshot> =>
        result.status === "fulfilled"
      )
      .map((result) => result.value);
    const setCookieHeaders = freezeStrings(fulfilled.flatMap((value) => value.setCookieHeaders ?? []));
    const rejected = providerResults.filter(
      (result): result is PromiseRejectedResult => result.status === "rejected"
    );
    if (rejected.length > 0) {
      const contractViolation = rejected.some((result) => result.reason instanceof ProviderContractError);
      const code: ProxyAuthResponseCode = contractViolation
        ? "auth_provider_contract_violation"
        : "auth_provider_unavailable";
      return responseResult(
        access,
        code,
        problemResponse(503, code, "authentication session refresh is unavailable", setCookieHeaders)
      );
    }

    const supabase = providerResults[0].status === "fulfilled"
      ? providerResults[0].value
      : unreachableProviderResult();
    const neonAuth = providerResults[1].status === "fulfilled"
      ? providerResults[1].value
      : unreachableProviderResult();
    const sessions: PairedProviderSessions = Object.freeze({ supabase, neonAuth });

    const hasSupabaseSession = supabase.session !== undefined;
    const hasNeonSession = neonAuth.session !== undefined;
    if (hasSupabaseSession !== hasNeonSession) {
      return responseResult(
        access,
        "auth_pair_incomplete",
        problemResponse(
          401,
          "auth_pair_incomplete",
          "Supabase and Neon Auth sessions must be established together",
          setCookieHeaders
        )
      );
    }

    let decision: SharedAuthProxyDecision;
    try {
      decision = normalizeSharedAuthDecision(
        await dependencies.verifyWithSharedAuth(sanitizedRequest, sessions)
      );
    } catch (error) {
      const contractViolation = error instanceof SharedAuthContractError;
      const code: ProxyAuthResponseCode = contractViolation
        ? "shared_auth_contract_violation"
        : "shared_auth_unavailable";
      return responseResult(
        access,
        code,
        problemResponse(503, code, "shared-auth verification is unavailable", setCookieHeaders)
      );
    }

    if (decision.kind === "denied") {
      return responseResult(
        access,
        "shared_auth_denied",
        problemResponse(
          decision.status ?? 403,
          "shared_auth_denied",
          "shared-auth denied the request",
          setCookieHeaders
        )
      );
    }

    if (!hasSupabaseSession && !hasNeonSession) {
      if (decision.kind === "authenticated") {
        return responseResult(
          access,
          "auth_pair_incomplete",
          problemResponse(
            401,
            "auth_pair_incomplete",
            "shared-auth identity is missing paired provider sessions",
            setCookieHeaders
          )
        );
      }
      return routeAnonymous(policy, access, sanitizedRequest, setCookieHeaders);
    }

    if (decision.kind !== "authenticated") {
      return responseResult(
        access,
        "shared_auth_rejected",
        problemResponse(
          401,
          "shared_auth_rejected",
          "shared-auth did not establish a canonical identity",
          setCookieHeaders
        )
      );
    }

    if (
      decision.bindings.supabase !== supabase.session?.subject ||
      decision.bindings["neon-auth"] !== neonAuth.session?.subject
    ) {
      return responseResult(
        access,
        "auth_evidence_mismatch",
        problemResponse(
          401,
          "auth_evidence_mismatch",
          "shared-auth provider bindings do not match the refreshed sessions",
          setCookieHeaders
        )
      );
    }

    const identity = canonicalIdentity(decision);
    const authenticatedRequest = attachCanonicalIdentityHeaders(sanitizedRequest, identity);
    return routeAuthenticated(
      policy,
      access,
      authenticatedRequest,
      identity,
      setCookieHeaders
    );
  };
}

function normalizePolicy(policy: ProxyAuthPolicy): NormalizedProxyPolicy {
  if (!policy || typeof policy !== "object") {
    throw new ProxyAuthConfigError("policy is required");
  }
  if (!Array.isArray(policy.routes)) {
    throw new ProxyAuthConfigError("policy.routes must be an array");
  }
  if (
    policy.defaultAccess !== "public" &&
    policy.defaultAccess !== "anonymous-only" &&
    policy.defaultAccess !== "authenticated-page" &&
    policy.defaultAccess !== "authenticated-api"
  ) {
    throw new ProxyAuthConfigError("policy.defaultAccess is invalid");
  }

  const seen = new Set<string>();
  const routes = policy.routes.map((rule, index) => {
    if (!rule || typeof rule !== "object") {
      throw new ProxyAuthConfigError(`policy.routes[${index}] must be an object`);
    }
    const path = safeApplicationPath(rule.path, `policy.routes[${index}].path`);
    if (rule.match !== "exact" && rule.match !== "prefix") {
      throw new ProxyAuthConfigError(`policy.routes[${index}].match is invalid`);
    }
    if (
      rule.access !== "ignore" &&
      rule.access !== "public" &&
      rule.access !== "anonymous-only" &&
      rule.access !== "authenticated-page" &&
      rule.access !== "authenticated-api"
    ) {
      throw new ProxyAuthConfigError(`policy.routes[${index}].access is invalid`);
    }
    const key = `${rule.match}:${path}`;
    if (seen.has(key)) {
      throw new ProxyAuthConfigError(`duplicate proxy route rule ${key}`);
    }
    seen.add(key);
    return Object.freeze({ path, match: rule.match, access: rule.access });
  });

  const returnToParameter = policy.returnToParameter ?? "returnTo";
  if (!/^[A-Za-z][A-Za-z0-9_-]{0,63}$/.test(returnToParameter)) {
    throw new ProxyAuthConfigError("policy.returnToParameter must be a bounded URL parameter name");
  }

  return Object.freeze({
    routes: Object.freeze(routes),
    defaultAccess: policy.defaultAccess,
    signInPath: safeApplicationPath(policy.signInPath, "policy.signInPath"),
    signedInPath: safeApplicationPath(policy.signedInPath, "policy.signedInPath"),
    returnToParameter
  });
}

function safeApplicationPath(value: unknown, label: string): string {
  if (typeof value !== "string" || value.length === 0 || value.length > MAX_PATH_BYTES) {
    throw new ProxyAuthConfigError(`${label} must be a non-empty bounded path`);
  }
  if (
    !value.startsWith("/") ||
    value.startsWith("//") ||
    value.includes("\\") ||
    value.includes("?") ||
    value.includes("#") ||
    /[\u0000-\u001f\u007f]/.test(value)
  ) {
    throw new ProxyAuthConfigError(`${label} must be a same-origin path without query or fragment`);
  }
  const parsed = new URL(value, "https://proxy.invalid");
  if (parsed.pathname !== value) {
    throw new ProxyAuthConfigError(`${label} must already be URL-normalized`);
  }
  return value;
}

function classifyRoute(policy: NormalizedProxyPolicy, pathname: string): ProxyRouteAccess {
  for (const rule of policy.routes) {
    if (routeMatches(rule, pathname)) return rule.access;
  }
  return policy.defaultAccess;
}

function routeMatches(rule: ProxyRouteRule, pathname: string): boolean {
  if (rule.match === "exact") return pathname === rule.path;
  if (rule.path === "/") return true;
  const prefix = rule.path.endsWith("/") ? rule.path : `${rule.path}/`;
  return pathname === rule.path || pathname.startsWith(prefix);
}

function stripCallerIdentityHeaders(request: Request): Request {
  const headers = new Headers(request.headers);
  for (const name of Array.from(headers.keys())) {
    const lower = name.toLowerCase();
    if (
      CALLER_IDENTITY_HEADERS.has(lower) ||
      CALLER_IDENTITY_HEADER_PREFIXES.some((prefix) => lower.startsWith(prefix))
    ) {
      headers.delete(name);
    }
  }
  return new Request(request, { headers });
}

function attachCanonicalIdentityHeaders(
  request: Request,
  identity: CanonicalProxyIdentity
): Request {
  const headers = new Headers(request.headers);
  headers.set(proxyIdentityHeaders.authority, identity.authority);
  headers.set(proxyIdentityHeaders.userId, identity.userId);
  headers.set(proxyIdentityHeaders.evidence, identity.providers.join(","));
  if (identity.tenantId) headers.set(proxyIdentityHeaders.tenantId, identity.tenantId);
  else headers.delete(proxyIdentityHeaders.tenantId);
  return new Request(request, { headers });
}

function normalizeProviderSnapshot(
  expectedProvider: PairedAuthProvider,
  value: ProviderSessionSnapshot
): ProviderSessionSnapshot {
  if (!value || typeof value !== "object") {
    throw new ProviderContractError(`${expectedProvider} resolver returned no snapshot`);
  }
  if (value.provider !== expectedProvider) {
    throw new ProviderContractError(
      `${expectedProvider} resolver returned evidence for an unsupported or different provider`
    );
  }
  const setCookieHeaders = normalizeSetCookieHeaders(value.setCookieHeaders);
  if (value.session === undefined) {
    return Object.freeze({ provider: expectedProvider, setCookieHeaders });
  }
  if (!value.session || typeof value.session !== "object") {
    throw new ProviderContractError(`${expectedProvider} session must be an object`);
  }
  const subject = boundedIdentifier(value.session.subject, `${expectedProvider} subject`, ProviderContractError);
  const tenantId = value.session.tenantId === undefined
    ? undefined
    : boundedIdentifier(value.session.tenantId, `${expectedProvider} tenant`, ProviderContractError);
  const expiresAtUnixMs = value.session.expiresAtUnixMs;
  if (
    expiresAtUnixMs !== undefined &&
    (!Number.isSafeInteger(expiresAtUnixMs) || expiresAtUnixMs <= 0)
  ) {
    throw new ProviderContractError(`${expectedProvider} expiry must be a positive safe integer`);
  }
  const session = Object.freeze({
    subject,
    ...(tenantId === undefined ? {} : { tenantId }),
    ...(expiresAtUnixMs === undefined ? {} : { expiresAtUnixMs })
  });
  return Object.freeze({ provider: expectedProvider, session, setCookieHeaders });
}

function normalizeSetCookieHeaders(value: readonly string[] | undefined): readonly string[] {
  if (value === undefined) return Object.freeze([] as string[]);
  if (!Array.isArray(value) || value.length > MAX_COOKIE_HEADERS) {
    throw new ProviderContractError("provider Set-Cookie mutations exceed the configured bound");
  }
  let total = 0;
  const normalized: string[] = [];
  for (const item of value) {
    if (
      typeof item !== "string" ||
      item.length === 0 ||
      item.length > MAX_COOKIE_HEADER_BYTES ||
      /[\r\n]/.test(item)
    ) {
      throw new ProviderContractError("provider Set-Cookie mutation is invalid");
    }
    total += item.length;
    if (total > MAX_COOKIE_TOTAL_BYTES) {
      throw new ProviderContractError("provider Set-Cookie mutations exceed the total byte bound");
    }
    normalized.push(item);
  }
  return freezeStrings(normalized);
}

function normalizeSharedAuthDecision(value: SharedAuthProxyDecision): SharedAuthProxyDecision {
  if (!value || typeof value !== "object") {
    throw new SharedAuthContractError("shared-auth returned no decision");
  }
  if (value.kind === "anonymous") return Object.freeze({ kind: "anonymous" });
  if (value.kind === "denied") {
    if (value.status !== undefined && value.status !== 401 && value.status !== 403) {
      throw new SharedAuthContractError("shared-auth denial status must be 401 or 403");
    }
    if (
      value.code !== undefined &&
      (typeof value.code !== "string" || !/^[a-z0-9][a-z0-9._-]{0,127}$/.test(value.code))
    ) {
      throw new SharedAuthContractError("shared-auth denial code is invalid");
    }
    return Object.freeze({
      kind: "denied",
      ...(value.status === undefined ? {} : { status: value.status }),
      ...(value.code === undefined ? {} : { code: value.code })
    });
  }
  if (value.kind !== "authenticated") {
    throw new SharedAuthContractError("shared-auth returned an unknown decision kind");
  }
  const userId = boundedIdentifier(value.userId, "shared-auth user ID", SharedAuthContractError);
  const tenantId = value.tenantId === undefined
    ? undefined
    : boundedIdentifier(value.tenantId, "shared-auth tenant ID", SharedAuthContractError);
  if (!value.bindings || typeof value.bindings !== "object") {
    throw new SharedAuthContractError("shared-auth provider bindings are required");
  }
  const bindings = Object.freeze({
    supabase: boundedIdentifier(
      value.bindings.supabase,
      "shared-auth Supabase binding",
      SharedAuthContractError
    ),
    "neon-auth": boundedIdentifier(
      value.bindings["neon-auth"],
      "shared-auth Neon Auth binding",
      SharedAuthContractError
    )
  });
  return Object.freeze({
    kind: "authenticated",
    userId,
    ...(tenantId === undefined ? {} : { tenantId }),
    bindings
  });
}

function boundedIdentifier<T extends Error>(
  value: unknown,
  label: string,
  ErrorType: new (message: string) => T
): string {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.length > MAX_IDENTIFIER_BYTES ||
    value.trim() !== value ||
    /[\u0000-\u001f\u007f]/.test(value)
  ) {
    throw new ErrorType(`${label} must be a non-empty bounded opaque string`);
  }
  return value;
}

function canonicalIdentity(
  decision: Extract<SharedAuthProxyDecision, { kind: "authenticated" }>
): CanonicalProxyIdentity {
  const identity = {
    authority: "shared-auth" as const,
    userId: decision.userId,
    ...(decision.tenantId === undefined ? {} : { tenantId: decision.tenantId }),
    providers: Object.freeze(["supabase", "neon-auth"] as const)
  };
  return Object.freeze(identity);
}

function routeAnonymous(
  policy: NormalizedProxyPolicy,
  access: DynamicProxyRouteAccess,
  request: Request,
  setCookieHeaders: readonly string[]
): ProxyAuthResult {
  if (access === "authenticated-api") {
    return responseResult(
      access,
      "authentication_required",
      problemResponse(
        401,
        "authentication_required",
        "authentication is required",
        setCookieHeaders
      )
    );
  }
  if (access === "authenticated-page") {
    const location = sameOriginRedirect(request, policy.signInPath);
    location.searchParams.set(policy.returnToParameter, safeReturnTo(request));
    return responseResult(
      access,
      "authentication_required",
      redirectResponse(location, setCookieHeaders)
    );
  }
  return nextResult(access, request, setCookieHeaders, undefined);
}

function routeAuthenticated(
  policy: NormalizedProxyPolicy,
  access: DynamicProxyRouteAccess,
  request: Request,
  identity: CanonicalProxyIdentity,
  setCookieHeaders: readonly string[]
): ProxyAuthResult {
  if (access === "anonymous-only") {
    return responseResult(
      access,
      "already_authenticated",
      redirectResponse(sameOriginRedirect(request, policy.signedInPath), setCookieHeaders)
    );
  }
  return nextResult(access, request, setCookieHeaders, identity);
}

function sameOriginRedirect(request: Request, path: string): URL {
  const current = new URL(request.url);
  const destination = new URL(path, current.origin);
  if (destination.origin !== current.origin) {
    throw new ProxyAuthConfigError("proxy redirect escaped the request origin");
  }
  return destination;
}

function safeReturnTo(request: Request): string {
  const url = new URL(request.url);
  const value = `${url.pathname}${url.search}`;
  return value.length <= MAX_PATH_BYTES ? value : url.pathname;
}

function nextResult(
  access: ProxyRouteAccess,
  request: Request,
  setCookieHeaders: readonly string[],
  identity: CanonicalProxyIdentity | undefined
): ProxyNextResult {
  return Object.freeze({
    kind: "next",
    access,
    request,
    ...(identity === undefined ? {} : { identity }),
    setCookieHeaders: freezeStrings(setCookieHeaders)
  });
}

function responseResult(
  access: DynamicProxyRouteAccess,
  code: ProxyAuthResponseCode,
  response: Response
): ProxyResponseResult {
  return Object.freeze({ kind: "response", access, code, response });
}

function problemResponse(
  status: number,
  code: ProxyAuthResponseCode,
  detail: string,
  setCookieHeaders: readonly string[]
): Response {
  const headers = authResponseHeaders(setCookieHeaders);
  headers.set("content-type", "application/problem+json");
  return new Response(
    JSON.stringify({
      type: `urn:ores:middleware:proxy-auth:${code}`,
      title: code,
      status,
      detail
    }),
    { status, headers }
  );
}

function redirectResponse(location: URL, setCookieHeaders: readonly string[]): Response {
  const headers = authResponseHeaders(setCookieHeaders);
  headers.set("location", location.toString());
  return new Response(null, { status: 307, headers });
}

function authResponseHeaders(setCookieHeaders: readonly string[]): Headers {
  const headers = new Headers({
    "cache-control": "no-store",
    vary: "authorization, cookie"
  });
  for (const value of setCookieHeaders) headers.append("set-cookie", value);
  return headers;
}

function freezeStrings(values: readonly string[]): readonly string[] {
  return Object.freeze([...new Set(values)]);
}

function unreachableProviderResult(): never {
  throw new Error("unreachable rejected provider result");
}
