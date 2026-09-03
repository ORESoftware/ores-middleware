package oresmiddleware

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"
)

func testConfig() Config {
	config := DefaultConfig("test")
	config.Settings.TLS.RequireHTTPS = false
	config.Settings.TLS.Mode = "disabled"
	config.Settings.RateLimit.Enabled = false
	return config
}

func TestProductionRejectsTestOnlyMiddleware(t *testing.T) {
	config := testConfig()
	config.Environment = Production
	config.Settings.FaultInjection.Enabled = true
	config.Settings.TestAuthBypass.Enabled = true
	issues := ValidateConfig(config)
	if len(issues) < 2 {
		t.Fatalf("expected production safety issues, got %#v", issues)
	}
}

func TestContextIsRequestScoped(t *testing.T) {
	value := RequestContext{RequestID: "r1", TraceID: "0123456789abcdef0123456789abcdef", Baggage: map[string]string{}}
	_, err := RunWithContext(context.Background(), value, func(ctx context.Context) (struct{}, error) {
		current, ok := CurrentContext(ctx)
		if !ok || current.RequestID != "r1" {
			t.Fatal("missing request context")
		}
		return struct{}{}, nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestStackAddsRequestAndSecurityHeaders(t *testing.T) {
	stack, err := New(testConfig(), Dependencies{})
	if err != nil {
		t.Fatal(err)
	}
	handler := stack.Wrap(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if _, ok := CurrentContext(request.Context()); !ok {
			t.Fatal("context not installed")
		}
		writer.Header().Set("Content-Type", "application/json")
		_, _ = writer.Write([]byte(`{"ok":true}`))
	}))
	request := httptest.NewRequest(http.MethodGet, "http://example.test/v1", nil)
	request.Header.Set("Accept", "application/json")
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	if response.Code != http.StatusOK {
		t.Fatalf("status %d: %s", response.Code, response.Body.String())
	}
	if response.Header().Get("x-request-id") == "" || response.Header().Get("traceparent") == "" {
		t.Fatal("missing correlation headers")
	}
	if response.Header().Get("x-content-type-options") != "nosniff" {
		t.Fatal("missing security headers")
	}
}

func TestDescriptorExportsStandardOperations(t *testing.T) {
	value := Descriptor()
	if len(value.OperationSymbols) != 7 {
		t.Fatalf("operations=%d", len(value.OperationSymbols))
	}
	if len(value.Capabilities) != len(Capabilities) {
		t.Fatal("capability mismatch")
	}
}
