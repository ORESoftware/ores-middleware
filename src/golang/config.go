package oresmiddleware

import (
	"fmt"
	"slices"
)

const ContractVersion = "1.0.0"

var Capabilities = []string{
	"request-context", "panic-recovery", "request-id", "trace-context", "structured-logging", "metrics-red", "deadline-timeout", "payload-limit", "rate-limit", "auth", "sync-observer", "json", "headers", "compression", "tls-policy", "security-headers", "idempotency", "ip-policy", "cache-etag", "content-negotiation", "fault-injection", "test-auth-bypass", "schema-capture",
}

type RuntimeEnvironment string

const (
	Development RuntimeEnvironment = "development"
	Test        RuntimeEnvironment = "test"
	Staging     RuntimeEnvironment = "staging"
	Production  RuntimeEnvironment = "production"
)

type IntegrationMode string

const (
	IntegrationDisabled IntegrationMode = "disabled"
	IntegrationHTTP     IntegrationMode = "http"
	IntegrationEmbedded IntegrationMode = "embedded"
)

type RateLimitPolicy struct {
	Enabled         bool     `json:"enabled"`
	Capacity        int      `json:"capacity"`
	RefillPerSecond float64  `json:"refillPerSecond"`
	KeyBy           []string `json:"keyBy"`
}

type CompressionPolicy struct {
	Enabled      bool     `json:"enabled"`
	MinimumBytes int      `json:"minimumBytes"`
	Algorithms   []string `json:"algorithms"`
}

type TLSPolicy struct {
	Mode                   string   `json:"mode"`
	RequireHTTPS           bool     `json:"requireHttps"`
	StrictForwardedHeaders bool     `json:"strictForwardedHeaders"`
	TrustedProxyCIDRs      []string `json:"trustedProxyCidrs"`
}

type SecurityHeaderPolicy struct {
	Enabled               bool   `json:"enabled"`
	HSTSMaxAgeSeconds     int64  `json:"hstsMaxAgeSeconds"`
	ContentSecurityPolicy string `json:"contentSecurityPolicy,omitempty"`
	FrameOptions          string `json:"frameOptions"`
}

type IdempotencyPolicy struct {
	Enabled         bool     `json:"enabled"`
	HeaderName      string   `json:"headerName"`
	TTLSeconds      int64    `json:"ttlSeconds"`
	RequiredMethods []string `json:"requiredMethods"`
}

type FaultInjectionPolicy struct {
	Enabled   bool    `json:"enabled"`
	LatencyMS int64   `json:"latencyMs"`
	ErrorRate float64 `json:"errorRate"`
	DropRate  float64 `json:"dropRate"`
}

type TestAuthBypassPolicy struct {
	Enabled      bool     `json:"enabled"`
	HeaderName   string   `json:"headerName"`
	AllowedCIDRs []string `json:"allowedCidrs"`
}

type MiddlewareSettings struct {
	RequestIDHeader          string               `json:"requestIdHeader"`
	TraceHeader              string               `json:"traceHeader"`
	TimeoutMS                int64                `json:"timeoutMs"`
	MaxBodyBytes             int64                `json:"maxBodyBytes"`
	ContextRegistryMaxEntries int                 `json:"contextRegistryMaxEntries"`
	ContextRegistryTTLMS     int64                `json:"contextRegistryTtlMs"`
	RateLimit                RateLimitPolicy      `json:"rateLimit"`
	Compression              CompressionPolicy    `json:"compression"`
	TLS                      TLSPolicy            `json:"tls"`
	SecurityHeaders          SecurityHeaderPolicy `json:"securityHeaders"`
	Idempotency              IdempotencyPolicy    `json:"idempotency"`
	FaultInjection           FaultInjectionPolicy `json:"faultInjection"`
	TestAuthBypass           TestAuthBypassPolicy `json:"testAuthBypass"`
	ContentRepresentations   []string             `json:"contentRepresentations"`
}

type SharedAuthIntegration struct {
	Mode             IntegrationMode `json:"mode"`
	Issuer           string          `json:"issuer,omitempty"`
	Audience         string          `json:"audience,omitempty"`
	JWKSURI          string          `json:"jwksUri,omitempty"`
	IntrospectionURL string          `json:"introspectionUrl,omitempty"`
	FailOpen         bool            `json:"failOpen"`
}

type OptoSyncIntegration struct {
	Mode        IntegrationMode `json:"mode"`
	Endpoint    string          `json:"endpoint,omitempty"`
	OutboxTopic string          `json:"outboxTopic,omitempty"`
	FailOpen    bool            `json:"failOpen"`
}

type OresOtelIntegration struct {
	Enabled          bool     `json:"enabled"`
	ServiceName      string   `json:"serviceName"`
	ExporterEndpoint string   `json:"exporterEndpoint,omitempty"`
	Propagators      []string `json:"propagators"`
}

type MiddlewareIntegrations struct {
	SharedAuth SharedAuthIntegration `json:"sharedAuth"`
	OptoSync   OptoSyncIntegration   `json:"optoSync"`
	OresOtel   OresOtelIntegration   `json:"oresOtel"`
}

type Config struct {
	ContractVersion      string                 `json:"contractVersion"`
	Environment          RuntimeEnvironment     `json:"environment"`
	RequiredCapabilities []string               `json:"requiredCapabilities"`
	Settings             MiddlewareSettings     `json:"settings"`
	Integrations         MiddlewareIntegrations `json:"integrations"`
}

type ValidationIssue struct {
	Path    string `json:"path"`
	Code    string `json:"code"`
	Message string `json:"message"`
}

type ValidationIssues []ValidationIssue

func (v ValidationIssues) Error() string { return fmt.Sprintf("middleware configuration has %d issue(s)", len(v)) }

func DefaultConfig(serviceName string) Config {
	return Config{
		ContractVersion: ContractVersion,
		Environment: Development,
		RequiredCapabilities: append([]string(nil), Capabilities...),
		Settings: MiddlewareSettings{
			RequestIDHeader: "x-request-id", TraceHeader: "traceparent", TimeoutMS: 5_000, MaxBodyBytes: 2 * 1024 * 1024,
			ContextRegistryMaxEntries: 10_000, ContextRegistryTTLMS: 30_000,
			RateLimit: RateLimitPolicy{Enabled: true, Capacity: 100, RefillPerSecond: 20, KeyBy: []string{"tenant", "user", "ip", "route"}},
			Compression: CompressionPolicy{Enabled: true, MinimumBytes: 1_024, Algorithms: []string{"gzip"}},
			TLS: TLSPolicy{Mode: "trusted-proxy", RequireHTTPS: true, StrictForwardedHeaders: true, TrustedProxyCIDRs: []string{"127.0.0.1/32", "::1/128"}},
			SecurityHeaders: SecurityHeaderPolicy{Enabled: true, HSTSMaxAgeSeconds: 31_536_000, ContentSecurityPolicy: "default-src 'self'; frame-ancestors 'none'", FrameOptions: "DENY"},
			Idempotency: IdempotencyPolicy{Enabled: true, HeaderName: "idempotency-key", TTLSeconds: 86_400, RequiredMethods: []string{"POST", "PUT", "PATCH"}},
			FaultInjection: FaultInjectionPolicy{},
			TestAuthBypass: TestAuthBypassPolicy{HeaderName: "x-test-auth-bypass", AllowedCIDRs: []string{"127.0.0.1/32", "::1/128"}},
			ContentRepresentations: []string{"application/json", "application/problem+json"},
		},
		Integrations: MiddlewareIntegrations{
			SharedAuth: SharedAuthIntegration{Mode: IntegrationDisabled, FailOpen: false},
			OptoSync: OptoSyncIntegration{Mode: IntegrationDisabled, FailOpen: true},
			OresOtel: OresOtelIntegration{Enabled: true, ServiceName: serviceName, Propagators: []string{"tracecontext", "baggage"}},
		},
	}
}

func ValidateConfig(config Config) ValidationIssues {
	var issues ValidationIssues
	add := func(path, code, message string) { issues = append(issues, ValidationIssue{Path: path, Code: code, Message: message}) }
	if config.ContractVersion != ContractVersion { add("/contractVersion", "unsupported_version", "expected "+ContractVersion) }
	if config.Settings.TimeoutMS <= 0 { add("/settings/timeoutMs", "range", "timeout must be positive") }
	if config.Settings.MaxBodyBytes <= 0 { add("/settings/maxBodyBytes", "range", "body limit must be positive") }
	if config.Settings.RateLimit.Enabled && (config.Settings.RateLimit.Capacity <= 0 || config.Settings.RateLimit.RefillPerSecond <= 0) { add("/settings/rateLimit", "invalid_rate_limit", "enabled token bucket requires positive capacity and refill") }
	if config.Settings.FaultInjection.ErrorRate < 0 || config.Settings.FaultInjection.ErrorRate > 1 || config.Settings.FaultInjection.DropRate < 0 || config.Settings.FaultInjection.DropRate > 1 { add("/settings/faultInjection", "range", "fault rates must be within 0..=1") }
	if config.Environment == Production && config.Settings.FaultInjection.Enabled { add("/settings/faultInjection/enabled", "production_forbidden", "fault injection is forbidden in production") }
	if config.Environment == Production && config.Settings.TestAuthBypass.Enabled { add("/settings/testAuthBypass/enabled", "production_forbidden", "test auth bypass is forbidden in production") }
	if config.Integrations.SharedAuth.FailOpen { add("/integrations/sharedAuth/failOpen", "auth_fail_open", "shared-auth must fail closed") }
	if config.Settings.TLS.Mode == "trusted-proxy" && len(config.Settings.TLS.TrustedProxyCIDRs) == 0 { add("/settings/tls/trustedProxyCidrs", "trusted_proxy_required", "trusted-proxy mode requires explicit CIDRs") }
	for _, capability := range config.RequiredCapabilities {
		if !slices.Contains(Capabilities, capability) { add("/requiredCapabilities", "unknown_capability", capability) }
	}
	return issues
}

type AdapterDescriptor struct {
	ContractVersion   string            `json:"contractVersion"`
	Language          string            `json:"language"`
	Runtime           string            `json:"runtime"`
	PackageName       string            `json:"packageName"`
	FrameworkAdapters []string          `json:"frameworkAdapters"`
	Capabilities      []string          `json:"capabilities"`
	OperationSymbols  map[string]string `json:"operationSymbols"`
}

func Descriptor() AdapterDescriptor {
	return AdapterDescriptor{
		ContractVersion: ContractVersion, Language: "golang", Runtime: "go-net-http", PackageName: "github.com/ORESoftware/ores-middleware/src/golang",
		FrameworkAdapters: []string{"net-http", "gorilla-mux", "gin", "echo", "fiber"}, Capabilities: append([]string(nil), Capabilities...),
		OperationSymbols: map[string]string{"descriptor": "Descriptor", "defaultConfig": "DefaultConfig", "validateConfig": "ValidateConfig", "createMiddleware": "New", "runWithContext": "RunWithContext", "currentContext": "CurrentContext", "capabilities": "Capabilities"},
	}
}
