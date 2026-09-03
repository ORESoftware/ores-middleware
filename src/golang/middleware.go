package oresmiddleware

import (
	"bytes"
	"compress/gzip"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"crypto/tls"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"math/rand/v2"
	"net"
	"net/http"
	"net/netip"
	"slices"
	"strings"
	"time"
)

type Stack struct {
	config Config
	deps Dependencies
	registry *ContextRegistry
}

func New(config Config, dependencies Dependencies) (*Stack, error) {
	if issues := ValidateConfig(config); len(issues) > 0 { return nil, issues }
	if dependencies.Now == nil { dependencies.Now = time.Now }
	if dependencies.RandomFloat64 == nil { dependencies.RandomFloat64 = rand.Float64 }
	if dependencies.RateLimiter == nil { dependencies.RateLimiter = NewMemoryTokenBucket(dependencies.Now) }
	if dependencies.IdempotencyStore == nil { dependencies.IdempotencyStore = NewMemoryIdempotencyStore(dependencies.Now) }
	return &Stack{config: config, deps: dependencies, registry: NewContextRegistry(config.Settings.ContextRegistryMaxEntries, time.Duration(config.Settings.ContextRegistryTTLMS)*time.Millisecond)}, nil
}

func (s *Stack) Config() Config { return s.config }

func (s *Stack) Wrap(next http.Handler) http.Handler {
	return http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		started := s.deps.Now()
		if request.ContentLength > s.config.Settings.MaxBodyBytes { writeProblem(writer, 413, "payload_too_large", "request body exceeds configured limit"); return }
		request.Body = http.MaxBytesReader(writer, request.Body, s.config.Settings.MaxBodyBytes)
		if !acceptsAny(request.Header.Get("Accept"), s.config.Settings.ContentRepresentations) { writeProblem(writer, 406, "not_acceptable", "no supported representation was requested"); return }
		trusted := trustedProxy(request.RemoteAddr, s.config.Settings.TLS.TrustedProxyCIDRs)
		forwardedProto := strings.ToLower(strings.TrimSpace(request.Header.Get("X-Forwarded-Proto")))
		if s.config.Settings.TLS.StrictForwardedHeaders && forwardedProto != "" && !trusted { writeProblem(writer, 400, "untrusted_forwarded_header", "forwarded transport headers came from an untrusted peer"); return }
		effectiveHTTPS := request.TLS != nil || (trusted && forwardedProto == "https")
		if s.config.Settings.TLS.RequireHTTPS && !effectiveHTTPS { writeProblem(writer, 426, "https_required", "HTTPS is required"); return }

		requestID := validToken(request.Header.Get(s.config.Settings.RequestIDHeader))
		if requestID == "" { requestID = randomHex(16) }
		traceID := parseTraceID(request.Header.Get(s.config.Settings.TraceHeader))
		if traceID == "" { traceID = randomHex(16) }
		value := RequestContext{RequestID: requestID, TraceID: traceID, Locale: request.Header.Get("Accept-Language"), StartedAtUnixMS: started.UnixMilli(), DeadlineUnixMS: started.Add(time.Duration(s.config.Settings.TimeoutMS)*time.Millisecond).UnixMilli(), Baggage: map[string]string{}}
		ctx, cancel := context.WithTimeout(request.Context(), time.Duration(s.config.Settings.TimeoutMS)*time.Millisecond)
		defer cancel()
		ctx = WithRequestContext(ctx, value)
		request = request.WithContext(ctx)

		if s.deps.IPAuthorizer != nil { allowed, err := s.deps.IPAuthorizer.Allow(ctx, request, value); if err != nil || !allowed { writeProblem(writer, 403, "ip_policy_denied", "request source is not permitted"); return } }
		bypass := s.config.Settings.TestAuthBypass.Enabled && request.Header.Get(s.config.Settings.TestAuthBypass.HeaderName) == "true"
		var decision AuthDecision
		var authErr error
		if bypass {
			if (s.config.Environment != Test && s.config.Environment != Staging) || s.deps.TestIdentity == nil { writeProblem(writer, 403, "test_bypass_denied", "test identity bypass is unavailable"); return }
			decision, authErr = s.deps.TestIdentity.Resolve(ctx, request, value)
		} else if s.deps.AuthVerifier != nil { decision, authErr = s.deps.AuthVerifier.Verify(ctx, request, value) }
		if authErr != nil { writeProblem(writer, 401, "authentication_failed", "authentication failed"); return }
		value.UserID, value.TenantID = decision.UserID, decision.TenantID
		for key, item := range decision.Claims { if strings.HasPrefix(key, "otel.") { value.Baggage[key] = item } }
		ctx = WithRequestContext(ctx, value); request = request.WithContext(ctx)
		if s.config.Integrations.SharedAuth.Mode != IntegrationDisabled && value.UserID == "" { writeProblem(writer, 401, "authentication_required", "shared-auth did not establish a user"); return }

		if s.config.Settings.RateLimit.Enabled {
			key := strings.Join([]string{value.TenantID, value.UserID, clientIP(request, trusted), request.URL.Path}, ":")
			allowed, err := s.deps.RateLimiter.Allow(ctx, key, s.config.Settings.RateLimit.Capacity, s.config.Settings.RateLimit.RefillPerSecond)
			if err != nil || !allowed { writeProblem(writer, 429, "rate_limited", "rate limit exceeded"); return }
		}
		if s.config.Settings.FaultInjection.Enabled {
			if delay := s.config.Settings.FaultInjection.LatencyMS; delay > 0 { select { case <-time.After(time.Duration(delay)*time.Millisecond): case <-ctx.Done(): writeProblem(writer, 504, "deadline_exceeded", "request deadline exceeded"); return } }
			if s.deps.RandomFloat64() < s.config.Settings.FaultInjection.DropRate { writeProblem(writer, 503, "fault_drop", "injected transport drop"); return }
			if s.deps.RandomFloat64() < s.config.Settings.FaultInjection.ErrorRate { writeProblem(writer, 500, "fault_error", "injected middleware error"); return }
		}

		idempotencyKey := ""
		if s.config.Settings.Idempotency.Enabled && slices.Contains(s.config.Settings.Idempotency.RequiredMethods, request.Method) {
			if header := request.Header.Get(s.config.Settings.Idempotency.HeaderName); header != "" { idempotencyKey = request.Method+":"+request.URL.Path+":"+header }
			if idempotencyKey != "" { if cached, ok, err := s.deps.IdempotencyStore.Get(ctx, idempotencyKey); err == nil && ok { copyResponse(writer, cached.Status, cached.Header, cached.Body); return } }
		}

		s.registry.Put(value); defer s.registry.Delete(value.RequestID)
		if s.deps.Telemetry != nil { s.deps.Telemetry.Started(ctx, value, request) }
		slog.InfoContext(ctx, "request started", "request_id", value.RequestID, "trace_id", value.TraceID, "method", request.Method, "path", request.URL.Path)
		capture := newBufferedResponse()
		done := make(chan any, 1)
		go func() {
			var panicValue any
			defer func() { if recovered := recover(); recovered != nil { panicValue = recovered }; done <- panicValue }()
			next.ServeHTTP(capture, request)
		}()
		var panicValue any
		select {
		case panicValue = <-done:
		case <-ctx.Done():
			capture = problemResponse(504, "deadline_exceeded", "request deadline exceeded")
		}
		if panicValue != nil { slog.ErrorContext(ctx, "request handler panic", "request_id", value.RequestID, "trace_id", value.TraceID); capture = problemResponse(500, "internal_error", "request handler failed") }
		status, headers, body := capture.snapshot()
		if request.Method == http.MethodGet && status == http.StatusOK && len(body) <= int(s.config.Settings.MaxBodyBytes) {
			digest := sha256.Sum256(body); etag := `"`+hex.EncodeToString(digest[:])+`"`; headers.Set("ETag", etag)
			if request.Header.Get("If-None-Match") == etag { status, body = http.StatusNotModified, nil }
		}
		applySecurityHeaders(s.config, headers)
		headers.Set(s.config.Settings.RequestIDHeader, value.RequestID)
		headers.Set("Traceparent", "00-"+value.TraceID+"-0000000000000000-01")
		headers.Add("Vary", "Accept")
		if shouldGzip(s.config, request, headers, body) { var output bytes.Buffer; compressor := gzip.NewWriter(&output); _, _ = compressor.Write(body); _ = compressor.Close(); body = output.Bytes(); headers.Set("Content-Encoding", "gzip"); headers.Del("Content-Length"); headers.Add("Vary", "Accept-Encoding") }
		duration := s.deps.Now().Sub(started)
		if s.deps.SchemaCapture != nil { if err := s.deps.SchemaCapture.Capture(ctx, request, status, headers.Clone(), append([]byte(nil), body...)); err != nil { slog.WarnContext(ctx, "schema capture failed", "request_id", value.RequestID) } }
		if s.deps.SyncObserver != nil { if err := s.deps.SyncObserver.Observe(ctx, value, request, status, duration); err != nil && !s.config.Integrations.OptoSync.FailOpen { status, headers, body = responseParts(problemResponse(503, "sync_observer_failed", "opto-sync observation failed")) } }
		if idempotencyKey != "" && status >= 200 && status < 300 { _ = s.deps.IdempotencyStore.Put(ctx, idempotencyKey, StoredResponse{Status: status, Header: headers, Body: body}, time.Duration(s.config.Settings.Idempotency.TTLSeconds)*time.Second) }
		if s.deps.Telemetry != nil { s.deps.Telemetry.Finished(ctx, value, request, status, duration) }
		slog.InfoContext(ctx, "request finished", "request_id", value.RequestID, "trace_id", value.TraceID, "status", status, "duration_ms", duration.Milliseconds())
		copyResponse(writer, status, headers, body)
	})
}

func DecodeJSON[T any](reader io.Reader, target *T) error {
	decoder := json.NewDecoder(reader); decoder.DisallowUnknownFields()
	if err := decoder.Decode(target); err != nil { return err }
	var extra any
	if err := decoder.Decode(&extra); !errors.Is(err, io.EOF) { return errors.New("JSON body must contain exactly one value") }
	return nil
}

func SecureTLSConfig() *tls.Config { return &tls.Config{MinVersion: tls.VersionTLS13} }

type bufferedResponse struct { header http.Header; status int; body bytes.Buffer }
func newBufferedResponse() *bufferedResponse { return &bufferedResponse{header: make(http.Header), status: http.StatusOK} }
func (r *bufferedResponse) Header() http.Header { return r.header }
func (r *bufferedResponse) WriteHeader(status int) { if r.status == http.StatusOK { r.status = status } }
func (r *bufferedResponse) Write(body []byte) (int, error) { return r.body.Write(body) }
func (r *bufferedResponse) snapshot() (int, http.Header, []byte) { return r.status, r.header.Clone(), append([]byte(nil), r.body.Bytes()...) }
func problemResponse(status int, code, detail string) *bufferedResponse { result := newBufferedResponse(); result.status = status; result.header.Set("Content-Type", "application/problem+json"); _ = json.NewEncoder(&result.body).Encode(map[string]any{"type": "urn:ores:middleware:"+code, "title": code, "status": status, "detail": detail}); return result }
func responseParts(value *bufferedResponse) (int, http.Header, []byte) { return value.snapshot() }
func writeProblem(writer http.ResponseWriter, status int, code, detail string) { value := problemResponse(status, code, detail); copyResponse(writer, value.snapshot()) }
func copyResponse(writer http.ResponseWriter, status int, headers http.Header, body []byte) { for key, values := range headers { for _, value := range values { writer.Header().Add(key, value) } }; writer.WriteHeader(status); if len(body) > 0 { _, _ = writer.Write(body) } }
func randomHex(size int) string { value := make([]byte, size); if _, err := rand.Read(value); err != nil { panic("secure random source unavailable") }; return hex.EncodeToString(value) }
func validToken(value string) string { if len(value) == 0 || len(value) > 128 { return "" }; for _, item := range value { if !(item >= 'a' && item <= 'z' || item >= 'A' && item <= 'Z' || item >= '0' && item <= '9' || strings.ContainsRune("._-", item)) { return "" } }; return value }
func parseTraceID(value string) string { parts := strings.Split(value, "-"); if len(parts) < 2 || len(parts[1]) != 32 { return "" }; if _, err := hex.DecodeString(parts[1]); err != nil { return "" }; return strings.ToLower(parts[1]) }
func acceptsAny(accept string, supported []string) bool { if accept == "" || accept == "*/*" { return true }; for _, value := range supported { if strings.Contains(accept, value) { return true } }; return false }
func trustedProxy(remote string, cidrs []string) bool { host, _, err := net.SplitHostPort(remote); if err != nil { host = remote }; address, err := netip.ParseAddr(host); if err != nil { return false }; for _, cidr := range cidrs { prefix, err := netip.ParsePrefix(cidr); if err == nil && prefix.Contains(address) { return true } }; return false }
func clientIP(request *http.Request, trusted bool) string { if trusted { if value := strings.TrimSpace(strings.Split(request.Header.Get("X-Forwarded-For"), ",")[0]); value != "" { return value }; if value := request.Header.Get("X-Real-IP"); value != "" { return value } }; host, _, err := net.SplitHostPort(request.RemoteAddr); if err == nil { return host }; return request.RemoteAddr }
func applySecurityHeaders(config Config, headers http.Header) { if !config.Settings.SecurityHeaders.Enabled { return }; headers.Set("X-Content-Type-Options", "nosniff"); headers.Set("X-Frame-Options", config.Settings.SecurityHeaders.FrameOptions); headers.Set("Referrer-Policy", "strict-origin-when-cross-origin"); headers.Set("Strict-Transport-Security", fmt.Sprintf("max-age=%d; includeSubDomains", config.Settings.SecurityHeaders.HSTSMaxAgeSeconds)); if value := config.Settings.SecurityHeaders.ContentSecurityPolicy; value != "" { headers.Set("Content-Security-Policy", value) } }
func shouldGzip(config Config, request *http.Request, headers http.Header, body []byte) bool { return config.Settings.Compression.Enabled && len(body) >= config.Settings.Compression.MinimumBytes && strings.Contains(request.Header.Get("Accept-Encoding"), "gzip") && headers.Get("Content-Encoding") == "" }
