/**
 * Provider-neutral authentication policy for framework proxy boundaries.
 *
 * Supabase and Neon Auth adapters refresh their own browser sessions and return
 * only normalized evidence plus opaque Set-Cookie values. shared-auth remains
 * the canonical identity authority and must bind both pieces of evidence before
 * any identity header is forwarded to an application handler.
 */
export declare const pairedAuthProviders: readonly ["supabase", "neon-auth"];
export type PairedAuthProvider = (typeof pairedAuthProviders)[number];
export declare const proxyIdentityHeaders: Readonly<{
    readonly authority: "x-ores-auth-authority";
    readonly userId: "x-ores-auth-user-id";
    readonly tenantId: "x-ores-auth-tenant-id";
    readonly evidence: "x-ores-auth-evidence";
}>;
export type ProxyRouteAccess = "ignore" | "public" | "anonymous-only" | "authenticated-page" | "authenticated-api";
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
export type SharedAuthProxyDecision = {
    readonly kind: "anonymous";
} | {
    readonly kind: "authenticated";
    readonly userId: string;
    readonly tenantId?: string;
    /** Exact provider subjects accepted and mapped by shared-auth. */
    readonly bindings: Readonly<Record<PairedAuthProvider, string>>;
} | {
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
    verifyWithSharedAuth(request: Request, sessions: PairedProviderSessions): Promise<SharedAuthProxyDecision>;
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
export type ProxyAuthResponseCode = "authentication_required" | "already_authenticated" | "auth_pair_incomplete" | "auth_provider_unavailable" | "auth_provider_contract_violation" | "shared_auth_unavailable" | "shared_auth_contract_violation" | "shared_auth_rejected" | "shared_auth_denied" | "auth_evidence_mismatch";
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
export declare class ProxyAuthConfigError extends Error {
    constructor(message: string);
}
/** Secure defaults: static framework assets are ignored and every app route is protected. */
export declare function defaultPairedAuthProxyPolicy(): ProxyAuthPolicy;
export declare function createPairedAuthProxy(options: PairedAuthProxyOptions): PairedAuthProxy;
//# sourceMappingURL=proxy-auth.d.ts.map