package oresmiddleware

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"sync"
	"testing"
	"time"
)

func TestShutdownRequestLeaseAccountsExactlyOnce(t *testing.T) {
	coordinator := DefaultShutdownCoordinator()
	first, rejection := coordinator.BeginRequest()
	if rejection != nil {
		t.Fatalf("first request rejected: %#v", rejection)
	}
	second, rejection := coordinator.BeginRequest()
	if rejection != nil {
		t.Fatalf("second request rejected: %#v", rejection)
	}
	if got := coordinator.ActiveRequests(); got != 2 {
		t.Fatalf("active requests = %d, want 2", got)
	}

	first.Finish()
	first.Finish()
	if got := coordinator.ActiveRequests(); got != 1 {
		t.Fatalf("active requests after idempotent finish = %d, want 1", got)
	}
	second.Finish()
	if got := coordinator.ActiveRequests(); got != 0 {
		t.Fatalf("active requests = %d, want 0", got)
	}
}

func TestShutdownConcurrentFinishAccountsExactlyOnce(t *testing.T) {
	coordinator := DefaultShutdownCoordinator()
	lease, rejection := coordinator.BeginRequest()
	if rejection != nil {
		t.Fatalf("request rejected: %#v", rejection)
	}

	var wait sync.WaitGroup
	for range 32 {
		wait.Add(1)
		go func() {
			defer wait.Done()
			lease.Finish()
		}()
	}
	wait.Wait()

	if got := coordinator.ActiveRequests(); got != 0 {
		t.Fatalf("active requests after concurrent finish = %d, want 0", got)
	}
}

func TestShutdownDrainWaitsForExistingRequest(t *testing.T) {
	coordinator := DefaultShutdownCoordinator()
	lease, rejection := coordinator.BeginRequest()
	if rejection != nil {
		t.Fatalf("request rejected: %#v", rejection)
	}

	outcomes := make(chan DrainOutcome, 1)
	go func() { outcomes <- coordinator.Drain() }()

	deadline := time.Now().Add(time.Second)
	for coordinator.Phase() != ShutdownDraining && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	if coordinator.Phase() != ShutdownDraining {
		t.Fatal("coordinator did not enter draining phase")
	}
	lease.Finish()

	select {
	case outcome := <-outcomes:
		if outcome.Kind != DrainCompleted || outcome.ActiveAtStart != 1 {
			t.Fatalf("unexpected outcome: %#v", outcome)
		}
	case <-time.After(time.Second):
		t.Fatal("drain did not complete")
	}
}

func TestShutdownDrainWithoutActiveRequestsCompletesImmediately(t *testing.T) {
	coordinator := DefaultShutdownCoordinator()
	outcome := coordinator.Drain()
	if outcome.Kind != DrainCompleted || outcome.ActiveAtStart != 0 {
		t.Fatalf("unexpected outcome: %#v", outcome)
	}
	if coordinator.Phase() != ShutdownDraining {
		t.Fatalf("phase = %v, want draining", coordinator.Phase())
	}
	if _, rejection := coordinator.BeginRequest(); rejection == nil {
		t.Fatal("draining coordinator must reject new work")
	}
}

func TestShutdownZeroTimeoutForcesInflightRequest(t *testing.T) {
	coordinator := NewShutdownCoordinator(0)
	lease, rejection := coordinator.BeginRequest()
	if rejection != nil {
		t.Fatalf("request rejected: %#v", rejection)
	}
	outcome := coordinator.Drain()
	if outcome.Kind != DrainTimedOut || outcome.Remaining != 1 {
		t.Fatalf("unexpected outcome: %#v", outcome)
	}
	if coordinator.Phase() != ShutdownForced {
		t.Fatalf("phase = %v, want forced", coordinator.Phase())
	}
	lease.Finish()
}

func TestShutdownForceInterruptsDrain(t *testing.T) {
	coordinator := NewShutdownCoordinator(time.Minute)
	lease, rejection := coordinator.BeginRequest()
	if rejection != nil {
		t.Fatalf("request rejected: %#v", rejection)
	}
	outcomes := make(chan DrainOutcome, 1)
	go func() { outcomes <- coordinator.Drain() }()

	deadline := time.Now().Add(time.Second)
	for coordinator.Phase() != ShutdownDraining && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	if !coordinator.ForceShutdown() {
		t.Fatal("first force transition should succeed")
	}
	if coordinator.ForceShutdown() {
		t.Fatal("second force transition should be idempotent")
	}

	select {
	case outcome := <-outcomes:
		if outcome.Kind != DrainForced || outcome.Remaining != 1 {
			t.Fatalf("unexpected outcome: %#v", outcome)
		}
	case <-time.After(time.Second):
		t.Fatal("forced drain did not complete")
	}
	lease.Finish()
}

func TestShutdownForceFromRunningRejectsNewWork(t *testing.T) {
	coordinator := NewShutdownCoordinator(time.Minute)
	lease, rejection := coordinator.BeginRequest()
	if rejection != nil {
		t.Fatalf("request rejected: %#v", rejection)
	}
	if !coordinator.ForceShutdown() {
		t.Fatal("force transition should succeed")
	}
	if _, rejection = coordinator.BeginRequest(); rejection == nil {
		t.Fatal("forced coordinator must reject new work")
	}
	outcome := coordinator.Drain()
	if outcome.Kind != DrainForced || outcome.Remaining != 1 {
		t.Fatalf("unexpected forced outcome: %#v", outcome)
	}
	lease.Finish()
}

func TestShutdownForceBeforeAnyWorkYieldsZeroRemaining(t *testing.T) {
	coordinator := DefaultShutdownCoordinator()
	if !coordinator.ForceShutdown() {
		t.Fatal("force transition should succeed")
	}
	outcome := coordinator.Drain()
	if outcome.Kind != DrainForced || outcome.Remaining != 0 {
		t.Fatalf("unexpected forced outcome: %#v", outcome)
	}
}

func TestShutdownRetryAfterRoundsUpAndStaysEndToEndOnly(t *testing.T) {
	coordinator := NewShutdownCoordinatorWithRetryAfter(5*time.Second, 1501*time.Millisecond)
	coordinator.StartDraining()
	rejection := coordinator.Rejection()
	if rejection.Status != ShutdownHTTPStatus {
		t.Fatalf("status = %d, want shutdown status %d", rejection.Status, ShutdownHTTPStatus)
	}
	if ShutdownHTTPStatus != http.StatusTooManyRequests {
		t.Fatalf("shutdown status = %d, want HTTP 429", ShutdownHTTPStatus)
	}
	if got := rejection.Headers["retry-after"]; got != "2" {
		t.Fatalf("retry-after = %q, want 2", got)
	}
	if _, found := rejection.Headers["connection"]; found {
		t.Fatal("generic rejection must not carry hop-by-hop Connection metadata")
	}
}

func TestShutdownRetryAfterSubsecondRoundsToOne(t *testing.T) {
	coordinator := NewShutdownCoordinatorWithRetryAfter(time.Second, time.Nanosecond)
	if got := coordinator.Rejection().Headers["retry-after"]; got != "1" {
		t.Fatalf("retry-after = %q, want 1", got)
	}
}

func TestShutdownHTTPMiddlewareRejectsHTTP1WithProblemDetails(t *testing.T) {
	coordinator := DefaultShutdownCoordinator()
	coordinator.StartDraining()
	handler := ShutdownHTTPMiddleware(coordinator, http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
		t.Fatal("downstream handler must not run while draining")
	}))

	request := httptest.NewRequest(http.MethodGet, "http://example.test/", nil)
	request.ProtoMajor = 1
	request.ProtoMinor = 1
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusTooManyRequests {
		t.Fatalf("status = %d, want 429", response.Code)
	}
	if got := response.Header().Get("Connection"); got != "close" {
		t.Fatalf("Connection = %q, want close", got)
	}
	if got := response.Header().Get("Retry-After"); got != "5" {
		t.Fatalf("Retry-After = %q, want 5", got)
	}
	if got := response.Header().Get("Cache-Control"); got != "no-store" {
		t.Fatalf("Cache-Control = %q, want no-store", got)
	}
	if got := response.Header().Get("Content-Type"); got != "application/problem+json" {
		t.Fatalf("Content-Type = %q, want application/problem+json", got)
	}
	if got := response.Header().Get("x-ores-error-code"); got != "service_draining" {
		t.Fatalf("x-ores-error-code = %q, want service_draining", got)
	}

	var problem map[string]any
	if err := json.Unmarshal(response.Body.Bytes(), &problem); err != nil {
		t.Fatalf("decode problem body: %v", err)
	}
	if problem["type"] != "about:blank" ||
		problem["title"] != "Request rejected" ||
		problem["code"] != "service_draining" ||
		problem["status"] != float64(429) {
		t.Fatalf("unexpected problem body: %#v", problem)
	}
}

func TestShutdownHTTPMiddlewareScopesConnectionHeaderToHTTP1(t *testing.T) {
	for _, testCase := range []struct {
		name       string
		protoMajor int
		protoMinor int
		connection string
	}{
		{name: "unknown", protoMajor: 0, protoMinor: 0, connection: ""},
		{name: "http-1-0", protoMajor: 1, protoMinor: 0, connection: "close"},
		{name: "http-1-1", protoMajor: 1, protoMinor: 1, connection: "close"},
		{name: "http-2", protoMajor: 2, protoMinor: 0, connection: ""},
		{name: "http-3", protoMajor: 3, protoMinor: 0, connection: ""},
	} {
		t.Run(testCase.name, func(t *testing.T) {
			coordinator := DefaultShutdownCoordinator()
			coordinator.StartDraining()
			handler := ShutdownHTTPMiddleware(coordinator, http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
				t.Fatal("downstream handler must not run while draining")
			}))
			request := httptest.NewRequest(http.MethodGet, "https://example.test/", nil)
			request.ProtoMajor = testCase.protoMajor
			request.ProtoMinor = testCase.protoMinor
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)
			if got := response.Header().Get("Connection"); got != testCase.connection {
				t.Fatalf("Connection = %q, want %q", got, testCase.connection)
			}
		})
	}
}

func TestShutdownHTTPMiddlewareOmitsConnectionForHTTP2(t *testing.T) {
	coordinator := DefaultShutdownCoordinator()
	coordinator.StartDraining()
	handler := ShutdownHTTPMiddleware(coordinator, http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
		t.Fatal("downstream handler must not run while draining")
	}))

	request := httptest.NewRequest(http.MethodGet, "https://example.test/", nil)
	request.ProtoMajor = 2
	request.ProtoMinor = 0
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusTooManyRequests {
		t.Fatalf("status = %d, want 429", response.Code)
	}
	if got := response.Header().Get("Connection"); got != "" {
		t.Fatalf("Connection must be omitted for HTTP/2, got %q", got)
	}
	if got := response.Header().Get("Retry-After"); got != "5" {
		t.Fatalf("Retry-After = %q, want 5", got)
	}
}

func TestShutdownHTTPMiddlewareReleasesLeaseAfterHandlerReturns(t *testing.T) {
	coordinator := DefaultShutdownCoordinator()
	handler := ShutdownHTTPMiddleware(coordinator, http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
		if got := coordinator.ActiveRequests(); got != 1 {
			t.Fatalf("active requests inside handler = %d, want 1", got)
		}
		writer.WriteHeader(http.StatusNoContent)
	}))

	response := httptest.NewRecorder()
	handler.ServeHTTP(response, httptest.NewRequest(http.MethodGet, "http://example.test/", nil))
	if response.Code != http.StatusNoContent {
		t.Fatalf("status = %d, want 204", response.Code)
	}
	if got := coordinator.ActiveRequests(); got != 0 {
		t.Fatalf("active requests after handler = %d, want 0", got)
	}
}

func TestShutdownConstructorRejectsInvalidDurations(t *testing.T) {
	assertPanics := func(name string, fn func()) {
		t.Helper()
		defer func() {
			if recover() == nil {
				t.Fatalf("%s did not panic", name)
			}
		}()
		fn()
	}
	assertPanics("negative drain timeout", func() { NewShutdownCoordinator(-time.Nanosecond) })
	assertPanics("zero retry-after", func() { NewShutdownCoordinatorWithRetryAfter(time.Second, 0) })
	assertPanics("negative retry-after", func() { NewShutdownCoordinatorWithRetryAfter(time.Second, -time.Nanosecond) })
}
