package oresmiddleware

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	nextloggers "github.com/ores-otel/ores.otel.log/sdk/go"
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
	if response.Header().Get("x-request-id") == "" {
		t.Fatal("missing request ID response header")
	}
	if response.Header().Get("traceparent") != "" {
		t.Fatal("middleware must not synthesize a response traceparent without a server span")
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

type lifecycleReport struct {
	failure      OperationFailure
	requestID    string
	userID       string
	logRequestID any
	logUserID    any
}

type panicFinishedTelemetry struct{}

func (panicFinishedTelemetry) Started(context.Context, RequestContext, *http.Request) {}
func (panicFinishedTelemetry) Finished(context.Context, RequestContext, *http.Request, int, time.Duration) {
	panic("private telemetry detail")
}

func TestAuthenticationPanicIsContainedInsideBaseRequestAndLogContext(t *testing.T) {
	reports := make(chan lifecycleReport, 1)
	stack, err := New(testConfig(), Dependencies{
		AuthVerifier: authVerifierFunc(func(ctx context.Context, _ *http.Request, value RequestContext) (AuthDecision, error) {
			current, ok := CurrentContext(ctx)
			if !ok || current.RequestID != value.RequestID {
				t.Fatalf("missing base request context: %#v", current)
			}
			logContext, ok := nextloggers.LogContextFrom(ctx)
			if !ok || logContext.Fields["request.id"] != value.RequestID {
				t.Fatalf("missing base ores-otel context: %#v", logContext)
			}
			panic("private authentication detail")
		}),
		OperationFailureReporter: func(ctx context.Context, failure OperationFailure) {
			current, _ := CurrentContext(ctx)
			logContext, _ := nextloggers.LogContextFrom(ctx)
			reports <- lifecycleReport{
				failure:      failure,
				requestID:    current.RequestID,
				userID:       current.UserID,
				logRequestID: logContext.Fields["request.id"],
				logUserID:    logContext.Fields["user.id"],
			}
		},
	})
	if err != nil {
		t.Fatal(err)
	}

	handler := stack.Wrap(http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
		t.Fatal("handler must not run")
	}))
	request := httptest.NewRequest(http.MethodGet, "http://example.test/profile", nil)
	request.Header.Set("Accept", "application/json")
	request.Header.Set("X-Request-ID", "auth-panic")
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusInternalServerError {
		t.Fatalf("status %d: %s", response.Code, response.Body.String())
	}
	if response.Header().Get("X-Request-ID") != "auth-panic" {
		t.Fatalf("missing request correlation: %#v", response.Header())
	}
	if strings.Contains(response.Body.String(), "private authentication detail") {
		t.Fatal("panic detail leaked into response")
	}
	report := <-reports
	if report.failure.Kind != OperationFailurePanic || report.failure.RequestID != "auth-panic" {
		t.Fatalf("unexpected failure: %#v", report.failure)
	}
	if report.requestID != "auth-panic" || report.logRequestID != "auth-panic" {
		t.Fatalf("reporter lost base context: %#v", report)
	}
}

func TestFinalizationPanicRetainsAuthenticatedActorContext(t *testing.T) {
	reports := make(chan lifecycleReport, 1)
	stack, err := New(testConfig(), Dependencies{
		AuthVerifier: authVerifierFunc(func(context.Context, *http.Request, RequestContext) (AuthDecision, error) {
			return AuthDecision{UserID: "user-42", TenantID: "tenant-7", Claims: map[string]string{"otel.plan": "pro", "private": "drop"}}, nil
		}),
		Telemetry: panicFinishedTelemetry{},
		OperationFailureReporter: func(ctx context.Context, failure OperationFailure) {
			current, _ := CurrentContext(ctx)
			logContext, _ := nextloggers.LogContextFrom(ctx)
			reports <- lifecycleReport{
				failure:      failure,
				requestID:    current.RequestID,
				userID:       current.UserID,
				logRequestID: logContext.Fields["request.id"],
				logUserID:    logContext.Fields["user.id"],
			}
		},
	})
	if err != nil {
		t.Fatal(err)
	}

	handler := stack.Wrap(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		current, ok := CurrentContext(request.Context())
		if !ok || current.UserID != "user-42" || current.TenantID != "tenant-7" {
			t.Fatalf("missing authenticated request context: %#v", current)
		}
		logContext, ok := nextloggers.LogContextFrom(request.Context())
		if !ok || logContext.Fields["user.id"] != "user-42" || logContext.Fields["tenant.id"] != "tenant-7" {
			t.Fatalf("missing authenticated ores-otel context: %#v", logContext)
		}
		writer.Header().Set("Content-Type", "application/json")
		_, _ = writer.Write([]byte(`{"ok":true}`))
	}))
	request := httptest.NewRequest(http.MethodGet, "http://example.test/profile", nil)
	request.Header.Set("Accept", "application/json")
	request.Header.Set("X-Request-ID", "finish-panic")
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusInternalServerError {
		t.Fatalf("status %d: %s", response.Code, response.Body.String())
	}
	if strings.Contains(response.Body.String(), "private telemetry detail") {
		t.Fatal("telemetry panic detail leaked into response")
	}
	report := <-reports
	if report.userID != "user-42" || report.logUserID != "user-42" {
		t.Fatalf("reporter lost authenticated actor context: %#v", report)
	}
}

func TestDeadlineSealsHandlerBufferAgainstLateWrites(t *testing.T) {
	config := testConfig()
	config.Settings.TimeoutMS = 5
	reports := make(chan OperationFailure, 1)
	stack, err := New(config, Dependencies{
		OperationFailureReporter: func(_ context.Context, failure OperationFailure) {
			reports <- failure
		},
	})
	if err != nil {
		t.Fatal(err)
	}

	lateWrite := make(chan error, 1)
	handler := stack.Wrap(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
		time.Sleep(25 * time.Millisecond)
		_, writeErr := writer.Write([]byte("late response must be rejected"))
		lateWrite <- writeErr
	}))
	request := httptest.NewRequest(http.MethodGet, "http://example.test/slow", nil)
	request.Header.Set("Accept", "application/json")
	request.Header.Set("X-Request-ID", "deadline-1")
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusGatewayTimeout {
		t.Fatalf("status %d: %s", response.Code, response.Body.String())
	}
	failure := <-reports
	if failure.Kind != OperationFailureDeadlineExceeded || failure.RequestID != "deadline-1" {
		t.Fatalf("unexpected timeout failure: %#v", failure)
	}
	select {
	case writeErr := <-lateWrite:
		if !errors.Is(writeErr, http.ErrHandlerTimeout) {
			t.Fatalf("late write error = %v", writeErr)
		}
	case <-time.After(250 * time.Millisecond):
		t.Fatal("late handler did not finish")
	}
	if strings.Contains(response.Body.String(), "late response") {
		t.Fatal("late handler write mutated completed response")
	}
}
