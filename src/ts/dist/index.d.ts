import { currentContext, runWithContext } from "./context.js";
import { checkRequestContract, type RequestContractValidator } from "./request-contract.js";
import { type OperationFailureReporter } from "./operation.js";
export { currentContext, runWithContext, checkRequestContract };
export type { RequestContractBody, RequestContractFailure, RequestContractIssue, RequestContractMatch, RequestContractValidationInput, RequestContractValidator } from "./request-contract.js";
export declare const contractVersion: "1.0.0";
export declare const capabilities: readonly ["request-context", "panic-recovery", "request-id", "trace-context", "structured-logging", "metrics-red", "deadline-timeout", "payload-limit", "rate-limit", "auth", "sync-observer", "json", "headers", "compression", "tls-policy", "security-headers", "idempotency", "ip-policy", "cache-etag", "content-negotiation", "fault-injection", "test-auth-bypass", "schema-capture"];
export type Capability = (typeof capabilities)[number];
export type RuntimeEnvironment = "development" | "test" | "staging" | "production";
export type IntegrationMode = "disabled" | "http" | "embedded";
export interface RequestContext {
    requestId: string;
    traceId: string;
    spanId?: string;
    tenantId?: string;
    userId?: string;
    locale?: string;
    startedAtUnixMs: number;
    deadlineUnixMs?: number;
    baggage: Record<string, string>;
}
export interface MiddlewareConfig {
    contractVersion: string;
    environment: RuntimeEnvironment;
    requiredCapabilities: string[];
    settings: {
        requestIdHeader: string;
        traceHeader: string;
        timeoutMs: number;
        maxBodyBytes: number;
        contextRegistryMaxEntries: number;
        contextRegistryTtlMs: number;
        rateLimit: {
            enabled: boolean;
            capacity: number;
            refillPerSecond: number;
            keyBy: Array<"ip" | "user" | "tenant" | "route">;
        };
        compression: {
            enabled: boolean;
            minimumBytes: number;
            algorithms: string[];
        };
        tls: {
            mode: "disabled" | "in-process" | "trusted-proxy";
            requireHttps: boolean;
            strictForwardedHeaders: boolean;
            trustedProxyCidrs: string[];
        };
        securityHeaders: {
            enabled: boolean;
            hstsMaxAgeSeconds: number;
            contentSecurityPolicy?: string;
            frameOptions: "DENY" | "SAMEORIGIN";
        };
        idempotency: {
            enabled: boolean;
            headerName: string;
            ttlSeconds: number;
            requiredMethods: string[];
        };
        faultInjection: {
            enabled: boolean;
            latencyMs: number;
            errorRate: number;
            dropRate: number;
        };
        testAuthBypass: {
            enabled: boolean;
            headerName: string;
            allowedCidrs: string[];
        };
        contentRepresentations: string[];
    };
    integrations: {
        sharedAuth: {
            mode: IntegrationMode;
            issuer?: string;
            audience?: string;
            jwksUri?: string;
            introspectionUrl?: string;
            failOpen: boolean;
        };
        optoSync: {
            mode: IntegrationMode;
            endpoint?: string;
            outboxTopic?: string;
            failOpen: boolean;
        };
        oresOtel: {
            enabled: boolean;
            serviceName: string;
            exporterEndpoint?: string;
            propagators: string[];
        };
    };
}
export interface ValidationIssue {
    path: string;
    code: string;
    message: string;
}
export interface AuthDecision {
    userId?: string;
    tenantId?: string;
    claims?: Record<string, string>;
}
export interface StoredResponse {
    status: number;
    headers: Array<[string, string]>;
    body: Uint8Array;
    expiresAt: number;
}
export interface MiddlewareDependencies {
    authVerifier?: (request: Request, context: RequestContext) => Promise<AuthDecision>;
    resolveTestIdentity?: (request: Request, context: RequestContext) => Promise<AuthDecision>;
    rateLimiter?: {
        allow(key: string, capacity: number, refillPerSecond: number): Promise<boolean>;
    };
    idempotencyStore?: {
        get(key: string): Promise<StoredResponse | undefined>;
        set(key: string, response: StoredResponse): Promise<void>;
    };
    isTrustedProxy?: (request: Request) => boolean;
    authorizeIp?: (request: Request, context: RequestContext) => Promise<boolean>;
    telemetry?: {
        started(context: RequestContext, request: Request): Promise<void> | void;
        finished(context: RequestContext, request: Request, response: Response, durationMs: number): Promise<void> | void;
    };
    /** Optional audited sink; defaults to the bounded ores-otel reporter. */
    operationFailureReporter?: OperationFailureReporter;
    /**
     * Strict parsed-request contract boundary. Operation resolution receives
     * method + pathname only; path/query/header/body data is validation-only.
     */
    requestContractValidator?: RequestContractValidator;
    syncObserver?: (context: RequestContext, request: Request, response: Response, durationMs: number) => Promise<void>;
    captureSchema?: (request: Request, response: Response) => Promise<void>;
    now?: () => number;
    random?: () => number;
}
export type NextHandler = (request: Request) => Promise<Response>;
export type PortableMiddleware = (request: Request, next: NextHandler) => Promise<Response>;
export declare class MiddlewareConfigError extends Error {
    readonly issues: ValidationIssue[];
    constructor(issues: ValidationIssue[]);
}
export declare function defaultConfig(serviceName: string): MiddlewareConfig;
export declare function validateConfig(config: MiddlewareConfig): ValidationIssue[];
export declare function createMiddleware(config: MiddlewareConfig, dependencies?: MiddlewareDependencies): PortableMiddleware;
export declare function readJson<T>(request: Request, validator?: (value: unknown) => value is T): Promise<T>;
export declare function sharedAuthHttpVerifier(config: MiddlewareConfig["integrations"]["sharedAuth"]): NonNullable<MiddlewareDependencies["authVerifier"]>;
export declare function optoSyncHttpObserver(config: MiddlewareConfig["integrations"]["optoSync"]): NonNullable<MiddlewareDependencies["syncObserver"]>;
export declare function descriptor(): {
    contractVersion: "1.0.0";
    language: string;
    runtime: string;
    packageName: string;
    frameworkAdapters: string[];
    capabilities: ("json" | "headers" | "request-context" | "panic-recovery" | "request-id" | "trace-context" | "structured-logging" | "metrics-red" | "deadline-timeout" | "payload-limit" | "rate-limit" | "auth" | "sync-observer" | "compression" | "tls-policy" | "security-headers" | "idempotency" | "ip-policy" | "cache-etag" | "content-negotiation" | "fault-injection" | "test-auth-bypass" | "schema-capture")[];
    operationSymbols: {
        descriptor: string;
        defaultConfig: string;
        validateConfig: string;
        createMiddleware: string;
        runWithContext: string;
        currentContext: string;
        capabilities: string;
    };
};
//# sourceMappingURL=index.d.ts.map