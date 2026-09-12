package oresmiddleware

import (
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

// Test-only verifier: application code must use its authenticated principal,
// never copy these synthetic headers into a production identity resolver.
type replayScopeTestVerifier struct{}

func (replayScopeTestVerifier) Verify(_ context.Context, request *http.Request, _ RequestContext) (AuthDecision, error) {
	return AuthDecision{TenantID: request.Header.Get("X-Test-Tenant"), UserID: request.Header.Get("X-Test-User")}, nil
}

func TestInstalledStackIsolatesReplayAndRefreshesCorrelation(t *testing.T) {
	store := NewMemoryIdempotencyStore(time.Now)
	var calls atomic.Int64
	makeHandler := func(service string) http.Handler {
		config := DefaultConfig(service)
		config.Environment = Test
		config.Settings.TLS.RequireHTTPS = false
		config.Settings.RateLimit.Enabled = false
		config.Settings.Compression.Enabled = false
		stack, err := New(config, Dependencies{AuthVerifier: replayScopeTestVerifier{}, IdempotencyStore: store})
		if err != nil {
			t.Fatal(err)
		}
		return stack.Wrap(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
			writer.Header().Set("Traceparent", "00-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-bbbbbbbbbbbbbbbb-01")
			writer.WriteHeader(http.StatusCreated)
			_, _ = fmt.Fprintf(writer, "%d", calls.Add(1))
		}))
	}
	handlers := map[string]http.Handler{"orders": makeHandler("orders"), "admin": makeHandler("admin")}
	invoke := func(service, tenant, user, target, requestID string) *httptest.ResponseRecorder {
		request := httptest.NewRequest(http.MethodPost, target, strings.NewReader("{}"))
		request.Header.Set("Idempotency-Key", "same-client-key")
		request.Header.Set("X-Test-Tenant", tenant)
		request.Header.Set("X-Test-User", user)
		request.Header.Set("X-Request-ID", requestID)
		writer := httptest.NewRecorder()
		handlers[service].ServeHTTP(writer, request)
		if writer.Code != http.StatusCreated {
			t.Fatalf("unexpected status %d: %s", writer.Code, writer.Body.String())
		}
		return writer
	}
	first := invoke("orders", "tenant-a", "user-a", "http://example.test/orders?x=1", "request-first")
	replay := invoke("orders", "tenant-a", "user-a", "http://example.test/orders?x=1", "request-replay")
	if first.Body.String() != replay.Body.String() || calls.Load() != 1 {
		t.Fatal("same-scope replay dispatched the handler again")
	}
	if replay.Header().Get("X-Request-ID") != "request-replay" || replay.Header().Get("Traceparent") != "" {
		t.Fatal("replay inherited stale correlation")
	}
	for index, item := range [][4]string{
		{"orders", "tenant-b", "user-a", "http://example.test/orders?x=1"},
		{"orders", "tenant-a", "user-b", "http://example.test/orders?x=1"},
		{"orders", "tenant-a", "user-a", "http://example.test/orders?x=2"},
		{"orders", "tenant-a", "user-a", "http://example.test/orders/other?x=1"},
		{"admin", "tenant-a", "user-a", "http://example.test/orders?x=1"},
	} {
		result := invoke(item[0], item[1], item[2], item[3], fmt.Sprintf("request-%d", index))
		if result.Body.String() == first.Body.String() {
			t.Fatal("different authenticated target scope replayed another response")
		}
	}
	if calls.Load() != 6 {
		t.Fatalf("expected six independently dispatched scopes, got %d", calls.Load())
	}
}
