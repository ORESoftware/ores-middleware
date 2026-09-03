package oresmiddleware

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	nextloggers "github.com/ores-otel/ores.otel.log/sdk/go"
)

type lockedLogTransport struct {
	mu      sync.Mutex
	records []nextloggers.LogRecord
}

func (transport *lockedLogTransport) Write(record nextloggers.LogRecord) error {
	transport.mu.Lock()
	defer transport.mu.Unlock()
	transport.records = append(transport.records, record)
	return nil
}

func (transport *lockedLogTransport) Snapshot() []nextloggers.LogRecord {
	transport.mu.Lock()
	defer transport.mu.Unlock()
	return append([]nextloggers.LogRecord(nil), transport.records...)
}

type failingLogTransport struct {
	mu     sync.Mutex
	writes int
}

func (transport *failingLogTransport) Write(nextloggers.LogRecord) error {
	transport.mu.Lock()
	transport.writes++
	transport.mu.Unlock()
	return errors.New("sink unavailable")
}

func (transport *failingLogTransport) WriteCount() int {
	transport.mu.Lock()
	defer transport.mu.Unlock()
	return transport.writes
}

func newTestOresLogger(name string, transport nextloggers.Transport) *nextloggers.Logger {
	return NewOresLogger(OresLoggerOptions{
		AppName:    "middleware-adversarial-test",
		Name:       name,
		Console:    false,
		Transports: []nextloggers.Transport{transport},
	})
}

func waitForLogRecord(t *testing.T, transport *lockedLogTransport, requestID, message string) nextloggers.LogRecord {
	t.Helper()
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		for _, record := range transport.Snapshot() {
			if record.Message == message && record.Fields["request.id"] == requestID {
				return record
			}
		}
		time.Sleep(5 * time.Millisecond)
	}
	t.Fatalf("missing log record request=%q message=%q in %#v", requestID, message, transport.Snapshot())
	return nextloggers.LogRecord{}
}

func TestOresLoggerParallelRequestsRemainIsolated(t *testing.T) {
	transport := &lockedLogTransport{}
	root := newTestOresLogger("server", transport)
	fileLogger := newTestOresLogger("orders-handler", transport)

	stack, err := New(testConfig(), Dependencies{
		AuthVerifier: authVerifierFunc(func(_ context.Context, request *http.Request, _ RequestContext) (AuthDecision, error) {
			return AuthDecision{
				UserID:   request.Header.Get("X-Test-User"),
				TenantID: request.Header.Get("X-Test-Tenant"),
				Claims: map[string]string{
					"otel.slot":    request.Header.Get("X-Test-Slot"),
					"authorization": "must-not-propagate",
				},
			}, nil
		}),
	})
	if err != nil {
		t.Fatal(err)
	}

	errorsFound := make(chan error, 128)
	handler := stack.WrapWithOresLogger(root, http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		slot := request.Header.Get("X-Test-Slot")
		requestID := "request-" + slot
		userID := "user-" + slot
		tenantID := "tenant-" + slot

		value, ok := CurrentContext(request.Context())
		if !ok || value.RequestID != requestID || value.UserID != userID || value.TenantID != tenantID {
			errorsFound <- fmt.Errorf("portable context mismatch for slot %s: %#v", slot, value)
			writer.WriteHeader(http.StatusInternalServerError)
			return
		}
		logContext, ok := nextloggers.LogContextFrom(request.Context())
		if !ok || logContext.Fields["request.id"] != requestID || logContext.LoggedInUser["id"] != userID {
			errorsFound <- fmt.Errorf("log context mismatch for slot %s: %#v", slot, logContext)
			writer.WriteHeader(http.StatusInternalServerError)
			return
		}
		if logContext.Baggage["otel.slot"] != slot {
			errorsFound <- fmt.Errorf("baggage mismatch for slot %s: %#v", slot, logContext.Baggage)
			writer.WriteHeader(http.StatusInternalServerError)
			return
		}
		if _, present := logContext.Baggage["authorization"]; present {
			errorsFound <- fmt.Errorf("unsafe baggage propagated for slot %s", slot)
			writer.WriteHeader(http.StatusInternalServerError)
			return
		}

		time.Sleep(time.Duration(len(slot)%5) * time.Millisecond)
		if err := fileLogger.InfoContext(request.Context(), "file:"+slot).Send(); err != nil {
			errorsFound <- fmt.Errorf("file logger slot %s: %w", slot, err)
		}
		if err := Log(request.Context()).Warn("request:" + slot); err != nil {
			errorsFound <- fmt.Errorf("request logger slot %s: %w", slot, err)
		}
		writer.WriteHeader(http.StatusNoContent)
	}))

	const requestCount = 64
	var waitGroup sync.WaitGroup
	for slot := 0; slot < requestCount; slot++ {
		waitGroup.Add(1)
		go func(slot int) {
			defer waitGroup.Done()
			request := httptest.NewRequest(http.MethodGet, fmt.Sprintf("http://example.test/orders/%d", slot), nil)
			request.Header.Set("Accept", "application/json")
			request.Header.Set("X-Request-ID", fmt.Sprintf("request-%d", slot))
			request.Header.Set("X-Test-Slot", fmt.Sprintf("%d", slot))
			request.Header.Set("X-Test-User", fmt.Sprintf("user-%d", slot))
			request.Header.Set("X-Test-Tenant", fmt.Sprintf("tenant-%d", slot))
			request.Header.Set("Traceparent", fmt.Sprintf("00-%032x-0123456789abcdef-01", slot))
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)
			if response.Code != http.StatusNoContent {
				errorsFound <- fmt.Errorf("slot %d returned %d: %s", slot, response.Code, response.Body.String())
			}
		}(slot)
	}
	waitGroup.Wait()
	close(errorsFound)
	for err := range errorsFound {
		t.Error(err)
	}
	if t.Failed() {
		return
	}

	records := transport.Snapshot()
	for slot := 0; slot < requestCount; slot++ {
		requestID := fmt.Sprintf("request-%d", slot)
		userID := fmt.Sprintf("user-%d", slot)
		tenantID := fmt.Sprintf("tenant-%d", slot)
		for _, message := range []string{fmt.Sprintf("file:%d", slot), fmt.Sprintf("request:%d", slot)} {
			matches := make([]nextloggers.LogRecord, 0, 1)
			for _, record := range records {
				if record.Message == message {
					matches = append(matches, record)
				}
			}
			if len(matches) != 1 {
				t.Fatalf("expected one %q record, got %d", message, len(matches))
			}
			record := matches[0]
			if record.Fields["request.id"] != requestID || record.Fields["user.id"] != userID || record.Fields["tenant.id"] != tenantID {
				t.Fatalf("cross-request contamination for %q: %#v", message, record)
			}
			if record.LoggedInUser["id"] != userID {
				t.Fatalf("wrong logged-in user for %q: %#v", message, record.LoggedInUser)
			}
			baggage, ok := record.Fields["otel.baggage"].(map[string]any)
			if !ok || baggage["otel.slot"] != fmt.Sprintf("%d", slot) {
				t.Fatalf("wrong baggage for %q: %#v", message, record.Fields["otel.baggage"])
			}
			if encoded := fmt.Sprintf("%#v", record); strings.Contains(strings.ToLower(encoded), "must-not-propagate") {
				t.Fatalf("unsafe claim reached log record %q: %s", message, encoded)
			}
		}
	}
}

func TestOresLoggerTransportFailureDoesNotAlterResponse(t *testing.T) {
	transport := &failingLogTransport{}
	root := newTestOresLogger("server", transport)
	stack, err := New(testConfig(), Dependencies{})
	if err != nil {
		t.Fatal(err)
	}
	handlerRan := false
	handler := stack.WrapWithOresLogger(root, http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
		handlerRan = true
		writer.WriteHeader(http.StatusNoContent)
	}))
	request := httptest.NewRequest(http.MethodGet, "http://example.test/failing-transport", nil)
	request.Header.Set("Accept", "application/json")
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if !handlerRan {
		t.Fatal("handler did not run after lifecycle transport failure")
	}
	if response.Code != http.StatusNoContent {
		t.Fatalf("transport failure changed response to %d: %s", response.Code, response.Body.String())
	}
	if transport.WriteCount() < 2 {
		t.Fatalf("expected start and completion delivery attempts, got %d", transport.WriteCount())
	}
}

func TestOresLoggerTimeoutIsNotReportedAsCompletion(t *testing.T) {
	transport := &lockedLogTransport{}
	root := newTestOresLogger("server", transport)
	config := testConfig()
	config.Settings.TimeoutMS = 15
	stack, err := New(config, Dependencies{})
	if err != nil {
		t.Fatal(err)
	}
	handlerFinished := make(chan struct{})
	handler := stack.WrapWithOresLogger(root, http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
		defer close(handlerFinished)
		time.Sleep(60 * time.Millisecond)
		writer.WriteHeader(http.StatusNoContent)
	}))
	request := httptest.NewRequest(http.MethodGet, "http://example.test/timeout", nil)
	request.Header.Set("Accept", "application/json")
	request.Header.Set("X-Request-ID", "request-timeout")
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusGatewayTimeout {
		t.Fatalf("expected 504, got %d: %s", response.Code, response.Body.String())
	}
	select {
	case <-handlerFinished:
	case <-time.After(2 * time.Second):
		t.Fatal("late handler did not finish")
	}
	waitForLogRecord(t, transport, "request-timeout", "request handler timed out")
	for _, record := range transport.Snapshot() {
		if record.Fields["request.id"] == "request-timeout" && record.Message == "request handler completed" {
			t.Fatalf("timed-out request emitted a completion record: %#v", record)
		}
	}
}

func TestOresLoggerPanicPreservesRecoveryAndPanicClassification(t *testing.T) {
	transport := &lockedLogTransport{}
	root := newTestOresLogger("server", transport)
	stack, err := New(testConfig(), Dependencies{})
	if err != nil {
		t.Fatal(err)
	}
	handler := stack.WrapWithOresLogger(root, http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
		panic("boom")
	}))
	request := httptest.NewRequest(http.MethodGet, "http://example.test/panic", nil)
	request.Header.Set("Accept", "application/json")
	request.Header.Set("X-Request-ID", "request-panic")
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusInternalServerError {
		t.Fatalf("expected 500, got %d: %s", response.Code, response.Body.String())
	}
	waitForLogRecord(t, transport, "request-panic", "request handler panic")
	for _, record := range transport.Snapshot() {
		if record.Fields["request.id"] == "request-panic" && record.Message == "request handler completed" {
			t.Fatalf("panicked request emitted a completion record: %#v", record)
		}
	}
}

func TestRequestLogWithoutMiddlewareContextFailsExplicitly(t *testing.T) {
	if err := Log(context.Background()).Info("outside request"); !errors.Is(err, ErrOresLoggerUnavailable) {
		t.Fatalf("expected ErrOresLoggerUnavailable, got %v", err)
	}
}
