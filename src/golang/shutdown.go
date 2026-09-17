package oresmiddleware

import (
	"encoding/json"
	"net/http"
	"strconv"
	"sync"
	"time"
)

const (
	DefaultDrainTimeout = 5 * time.Second
	DefaultRetryAfter   = 5 * time.Second
)

type ShutdownPhase uint8

const (
	ShutdownRunning ShutdownPhase = iota
	ShutdownDraining
	ShutdownForced
)

type ShutdownRejection struct {
	Status  int
	Code    string
	Message string
	Headers map[string]string
}

type DrainOutcomeKind uint8

const (
	DrainCompleted DrainOutcomeKind = iota
	DrainTimedOut
	DrainForced
)

type DrainOutcome struct {
	Kind          DrainOutcomeKind
	ActiveAtStart int
	Remaining     int
}

type ShutdownCoordinator struct {
	mu           sync.Mutex
	phase        ShutdownPhase
	active       int
	changed      chan struct{}
	drainTimeout time.Duration
	retryAfter   time.Duration
}

type DrainLease struct {
	once        sync.Once
	coordinator *ShutdownCoordinator
}

func NewShutdownCoordinator(drainTimeout time.Duration) *ShutdownCoordinator {
	return NewShutdownCoordinatorWithRetryAfter(drainTimeout, DefaultRetryAfter)
}

func NewShutdownCoordinatorWithRetryAfter(
	drainTimeout time.Duration,
	retryAfter time.Duration,
) *ShutdownCoordinator {
	if drainTimeout < 0 {
		panic("oresmiddleware: drain timeout must be non-negative")
	}
	if retryAfter <= 0 {
		panic("oresmiddleware: retry-after must be positive")
	}
	return &ShutdownCoordinator{
		phase:        ShutdownRunning,
		changed:      make(chan struct{}),
		drainTimeout: drainTimeout,
		retryAfter:   retryAfter,
	}
}

func DefaultShutdownCoordinator() *ShutdownCoordinator {
	return NewShutdownCoordinator(DefaultDrainTimeout)
}

func (coordinator *ShutdownCoordinator) Phase() ShutdownPhase {
	coordinator.mu.Lock()
	defer coordinator.mu.Unlock()
	return coordinator.phase
}

func (coordinator *ShutdownCoordinator) ActiveRequests() int {
	coordinator.mu.Lock()
	defer coordinator.mu.Unlock()
	return coordinator.active
}

func (coordinator *ShutdownCoordinator) IsAcceptingRequests() bool {
	return coordinator.Phase() == ShutdownRunning
}

func (coordinator *ShutdownCoordinator) BeginRequest() (*DrainLease, *ShutdownRejection) {
	coordinator.mu.Lock()
	defer coordinator.mu.Unlock()
	if coordinator.phase != ShutdownRunning {
		rejection := coordinator.rejectionLocked()
		return nil, &rejection
	}
	coordinator.active++
	return &DrainLease{coordinator: coordinator}, nil
}

func (lease *DrainLease) Finish() {
	if lease == nil || lease.coordinator == nil {
		return
	}
	lease.once.Do(func() {
		coordinator := lease.coordinator
		coordinator.mu.Lock()
		defer coordinator.mu.Unlock()
		if coordinator.active <= 0 {
			panic("oresmiddleware: shutdown request accounting underflow")
		}
		coordinator.active--
		coordinator.notifyLocked()
	})
}

func (coordinator *ShutdownCoordinator) StartDraining() bool {
	coordinator.mu.Lock()
	defer coordinator.mu.Unlock()
	if coordinator.phase != ShutdownRunning {
		return false
	}
	coordinator.phase = ShutdownDraining
	coordinator.notifyLocked()
	return true
}

func (coordinator *ShutdownCoordinator) ForceShutdown() bool {
	coordinator.mu.Lock()
	defer coordinator.mu.Unlock()
	if coordinator.phase == ShutdownForced {
		return false
	}
	coordinator.phase = ShutdownForced
	coordinator.notifyLocked()
	return true
}

func (coordinator *ShutdownCoordinator) Drain() DrainOutcome {
	coordinator.StartDraining()

	coordinator.mu.Lock()
	activeAtStart := coordinator.active
	if coordinator.phase == ShutdownForced {
		coordinator.mu.Unlock()
		return DrainOutcome{Kind: DrainForced, Remaining: activeAtStart}
	}
	if activeAtStart == 0 {
		coordinator.mu.Unlock()
		return DrainOutcome{Kind: DrainCompleted, ActiveAtStart: 0}
	}
	coordinator.mu.Unlock()

	timer := time.NewTimer(coordinator.drainTimeout)
	defer timer.Stop()

	for {
		coordinator.mu.Lock()
		if coordinator.active == 0 {
			coordinator.mu.Unlock()
			return DrainOutcome{Kind: DrainCompleted, ActiveAtStart: activeAtStart}
		}
		if coordinator.phase == ShutdownForced {
			remaining := coordinator.active
			coordinator.mu.Unlock()
			return DrainOutcome{Kind: DrainForced, Remaining: remaining}
		}
		changed := coordinator.changed
		coordinator.mu.Unlock()

		select {
		case <-changed:
			continue
		case <-timer.C:
			coordinator.mu.Lock()
			if coordinator.active == 0 {
				coordinator.mu.Unlock()
				return DrainOutcome{Kind: DrainCompleted, ActiveAtStart: activeAtStart}
			}
			remaining := coordinator.active
			coordinator.phase = ShutdownForced
			coordinator.notifyLocked()
			coordinator.mu.Unlock()
			return DrainOutcome{Kind: DrainTimedOut, Remaining: remaining}
		}
	}
}

func (coordinator *ShutdownCoordinator) Rejection() ShutdownRejection {
	coordinator.mu.Lock()
	defer coordinator.mu.Unlock()
	return coordinator.rejectionLocked()
}

func (coordinator *ShutdownCoordinator) rejectionLocked() ShutdownRejection {
	retryAfterSeconds := int64((coordinator.retryAfter + time.Second - 1) / time.Second)
	if retryAfterSeconds < 1 {
		retryAfterSeconds = 1
	}
	return ShutdownRejection{
		Status:  http.StatusServiceUnavailable,
		Code:    "service_draining",
		Message: "service is draining and is not accepting new requests",
		Headers: map[string]string{
			"connection":  "close",
			"retry-after": strconv.FormatInt(retryAfterSeconds, 10),
		},
	}
}

func (coordinator *ShutdownCoordinator) notifyLocked() {
	close(coordinator.changed)
	coordinator.changed = make(chan struct{})
}

// ShutdownHTTPMiddleware rejects requests admitted after draining begins while
// retaining a lease for requests already inside the handler chain. Consumers
// still own signal policy, listener shutdown, middleware ordering, telemetry
// flushing, and final process termination.
func ShutdownHTTPMiddleware(
	coordinator *ShutdownCoordinator,
	next http.Handler,
) http.Handler {
	if coordinator == nil {
		panic("oresmiddleware: shutdown coordinator must not be nil")
	}
	if next == nil {
		panic("oresmiddleware: shutdown next handler must not be nil")
	}

	return http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		lease, rejection := coordinator.BeginRequest()
		if rejection != nil {
			writeShutdownRejection(writer, request, *rejection)
			return
		}
		defer lease.Finish()
		next.ServeHTTP(writer, request)
	})
}

func writeShutdownRejection(
	writer http.ResponseWriter,
	request *http.Request,
	rejection ShutdownRejection,
) {
	writer.Header().Set("Content-Type", "application/problem+json")
	writer.Header().Set("Cache-Control", "no-store")
	writer.Header().Set("Retry-After", rejection.Headers["retry-after"])
	writer.Header().Set("X-Ores-Error-Code", rejection.Code)
	if request.ProtoMajor <= 1 {
		writer.Header().Set("Connection", "close")
	}
	writer.WriteHeader(rejection.Status)
	_ = json.NewEncoder(writer).Encode(map[string]any{
		"status": rejection.Status,
		"code":   rejection.Code,
		"title":  "Request rejected",
	})
}
