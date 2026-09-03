package oresmiddleware

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"

	nextloggers "github.com/ores-otel/ores.otel.log/sdk/go"
)

type authVerifierFunc func(context.Context, *http.Request, RequestContext) (AuthDecision, error)

func (verify authVerifierFunc) Verify(ctx context.Context, request *http.Request, value RequestContext) (AuthDecision, error) {
	return verify(ctx, request, value)
}

func TestOresLoggerIsPinnedToAuthenticatedRequestContext(t *testing.T) {
	transport := &nextloggers.MemoryTransport{}
	root := NewOresLogger(OresLoggerOptions{
		AppName:    "middleware-test",
		Name:       "server",
		Console:    false,
		Transports: []nextloggers.Transport{transport},
	})
	fileLogger := NewOresLogger(OresLoggerOptions{
		AppName:    "middleware-test",
		Name:       "orders-handler",
		Console:    false,
		Transports: []nextloggers.Transport{transport},
	})

	stack, err := New(testConfig(), Dependencies{
		AuthVerifier: authVerifierFunc(func(context.Context, *http.Request, RequestContext) (AuthDecision, error) {
			return AuthDecision{UserID: "user-42", TenantID: "tenant-7"}, nil
		}),
	})
	if err != nil {
		t.Fatal(err)
	}

	handler := stack.WrapWithOresLogger(root, http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		value, ok := CurrentContext(request.Context())
		if !ok || value.RequestID != "request-42" || value.UserID != "user-42" {
			t.Fatalf("unexpected portable context: %#v", value)
		}
		requestLogger, ok := OresLoggerFromContext(request.Context())
		if !ok || requestLogger == root {
			t.Fatal("request-specific child logger was not installed")
		}
		logContext, ok := nextloggers.LogContextFrom(request.Context())
		if !ok || logContext.Fields["request.id"] != "request-42" || logContext.Fields["tenant.id"] != "tenant-7" {
			t.Fatalf("unexpected ores log context: %#v", logContext)
		}
		if logContext.LoggedInUser["id"] != "user-42" {
			t.Fatalf("missing authenticated user: %#v", logContext.LoggedInUser)
		}
		if err := Log(request.Context()).Warn("slow dependency"); err != nil {
			t.Fatal(err)
		}
		if err := fileLogger.InfoContext(request.Context(), "handler reached").Send(); err != nil {
			t.Fatal(err)
		}
		writer.Header().Set("Content-Type", "application/json")
		_, _ = writer.Write([]byte(`{"ok":true}`))
	}))

	request := httptest.NewRequest(http.MethodGet, "http://example.test/orders/42", nil)
	request.Header.Set("Accept", "application/json")
	request.Header.Set("X-Request-ID", "request-42")
	request.Header.Set("Traceparent", "00-0123456789abcdef0123456789abcdef-0123456789abcdef-01")
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	if response.Code != http.StatusOK {
		t.Fatalf("status %d: %s", response.Code, response.Body.String())
	}

	found := map[string]bool{}
	for _, record := range transport.Records {
		found[record.Message] = true
		if record.Message == "slow dependency" {
			if record.Fields["request.id"] != "request-42" || record.LoggedInUser["id"] != "user-42" {
				t.Fatalf("request logger lost correlation: %#v", record)
			}
		}
		if record.Message == "handler reached" && record.Fields["tenant.id"] != "tenant-7" {
			t.Fatalf("file logger lost request context: %#v", record)
		}
	}
	for _, message := range []string{"request handler started", "slow dependency", "handler reached", "request handler completed"} {
		if !found[message] {
			t.Fatalf("missing log record %q in %#v", message, transport.Records)
		}
	}
}
