package oresmiddleware

import (
	"net/http"
	"net/http/httptest"
	"sync"
	"testing"
	"time"
)

func TestShutdownManyConcurrentLeaseReleasesCompleteDrain(t *testing.T) {
	coordinator := NewShutdownCoordinator(2 * time.Second)
	leases := make([]*DrainLease, 0, 64)
	for range 64 {
		lease, rejection := coordinator.BeginRequest()
		if rejection != nil {
			t.Fatalf("request rejected: %#v", rejection)
		}
		leases = append(leases, lease)
	}
	if !coordinator.StartDraining() {
		t.Fatal("first drain transition should succeed")
	}

	outcomes := make(chan DrainOutcome, 1)
	go func() { outcomes <- coordinator.Drain() }()

	var releases sync.WaitGroup
	for _, lease := range leases {
		releases.Add(1)
		go func(lease *DrainLease) {
			defer releases.Done()
			lease.Finish()
		}(lease)
	}
	releases.Wait()

	select {
	case outcome := <-outcomes:
		if outcome.Kind != DrainCompleted {
			t.Fatalf("unexpected outcome: %#v", outcome)
		}
	case <-time.After(time.Second):
		t.Fatal("drain did not complete after concurrent releases")
	}
	if got := coordinator.ActiveRequests(); got != 0 {
		t.Fatalf("active requests = %d, want 0", got)
	}
}

func TestShutdownConcurrentDrainWaitersAreAllReleased(t *testing.T) {
	coordinator := NewShutdownCoordinator(2 * time.Second)
	lease, rejection := coordinator.BeginRequest()
	if rejection != nil {
		t.Fatalf("request rejected: %#v", rejection)
	}
	coordinator.StartDraining()

	outcomes := make(chan DrainOutcome, 2)
	go func() { outcomes <- coordinator.Drain() }()
	go func() { outcomes <- coordinator.Drain() }()
	lease.Finish()

	for range 2 {
		select {
		case outcome := <-outcomes:
			if outcome.Kind != DrainCompleted {
				t.Fatalf("unexpected outcome: %#v", outcome)
			}
		case <-time.After(time.Second):
			t.Fatal("one of the drain waiters was not released")
		}
	}
}

func TestShutdownHTTPMiddlewareOmitsConnectionForHTTP3(t *testing.T) {
	coordinator := DefaultShutdownCoordinator()
	coordinator.StartDraining()
	handler := ShutdownHTTPMiddleware(coordinator, http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
		t.Fatal("downstream handler must not run while draining")
	}))

	request := httptest.NewRequest(http.MethodGet, "https://example.test/", nil)
	request.ProtoMajor = 3
	request.ProtoMinor = 0
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusTooManyRequests {
		t.Fatalf("status = %d, want 429", response.Code)
	}
	if got := response.Header().Get("Connection"); got != "" {
		t.Fatalf("Connection must be omitted for HTTP/3, got %q", got)
	}
}

func TestShutdownHTTPMiddlewareReleasesLeaseWhenDownstreamPanics(t *testing.T) {
	coordinator := DefaultShutdownCoordinator()
	handler := ShutdownHTTPMiddleware(coordinator, http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
		panic("synthetic downstream panic")
	}))

	func() {
		defer func() {
			if recover() == nil {
				t.Fatal("expected downstream panic")
			}
		}()
		handler.ServeHTTP(httptest.NewRecorder(), httptest.NewRequest(http.MethodGet, "http://example.test/", nil))
	}()

	if got := coordinator.ActiveRequests(); got != 0 {
		t.Fatalf("active requests after panic = %d, want 0", got)
	}
}

func TestShutdownTimeoutForceCannotRegressAfterLateLeaseRelease(t *testing.T) {
	coordinator := NewShutdownCoordinator(0)
	lease, rejection := coordinator.BeginRequest()
	if rejection != nil {
		t.Fatalf("request rejected: %#v", rejection)
	}

	outcome := coordinator.Drain()
	if outcome.Kind != DrainTimedOut || outcome.Remaining != 1 {
		t.Fatalf("unexpected timeout outcome: %#v", outcome)
	}
	if coordinator.Phase() != ShutdownForced {
		t.Fatalf("phase = %v, want forced", coordinator.Phase())
	}
	if coordinator.StartDraining() {
		t.Fatal("forced coordinator must not regress to draining")
	}
	if coordinator.ForceShutdown() {
		t.Fatal("force transition should be idempotent")
	}

	lease.Finish()
	if got := coordinator.ActiveRequests(); got != 0 {
		t.Fatalf("active requests after late release = %d, want 0", got)
	}
	if _, rejection = coordinator.BeginRequest(); rejection == nil {
		t.Fatal("forced coordinator must remain closed to new work")
	}
}
