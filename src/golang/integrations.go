package oresmiddleware

import (
	"context"
	"net/http"
	"sync"
	"time"
)

type AuthDecision struct {
	UserID   string
	TenantID string
	Claims   map[string]string
}

type AuthVerifier interface {
	Verify(context.Context, *http.Request, RequestContext) (AuthDecision, error)
}
type TestIdentityResolver interface {
	Resolve(context.Context, *http.Request, RequestContext) (AuthDecision, error)
}
type IPAuthorizer interface {
	Allow(context.Context, *http.Request, RequestContext) (bool, error)
}
type SyncObserver interface {
	Observe(context.Context, RequestContext, *http.Request, int, time.Duration) error
}
type TelemetrySink interface {
	Started(context.Context, RequestContext, *http.Request)
	Finished(context.Context, RequestContext, *http.Request, int, time.Duration)
}
type SchemaCapture interface {
	Capture(context.Context, *http.Request, int, http.Header, []byte) error
}
type RateLimiter interface {
	Allow(context.Context, string, int, float64) (bool, error)
}
type IdempotencyStore interface {
	Get(context.Context, string) (StoredResponse, bool, error)
	Put(context.Context, string, StoredResponse, time.Duration) error
}

type StoredResponse struct {
	Status    int
	Header    http.Header
	Body      []byte
	ExpiresAt time.Time
}

type Dependencies struct {
	AuthVerifier             AuthVerifier
	TestIdentity             TestIdentityResolver
	IPAuthorizer             IPAuthorizer
	SyncObserver             SyncObserver
	Telemetry                TelemetrySink
	SchemaCapture            SchemaCapture
	OperationFailureReporter OperationFailureReporter
	RateLimiter              RateLimiter
	IdempotencyStore         IdempotencyStore
	Now                      func() time.Time
	RandomFloat64            func() float64
}

type bucket struct {
	tokens  float64
	updated time.Time
}
type MemoryTokenBucket struct {
	mu      sync.Mutex
	buckets map[string]bucket
	now     func() time.Time
}

func NewMemoryTokenBucket(now func() time.Time) *MemoryTokenBucket {
	return &MemoryTokenBucket{buckets: make(map[string]bucket), now: now}
}

func (m *MemoryTokenBucket) Allow(_ context.Context, key string, capacity int, refill float64) (bool, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	now := m.now()
	value, ok := m.buckets[key]
	if !ok {
		value = bucket{tokens: float64(capacity), updated: now}
	}
	value.tokens += now.Sub(value.updated).Seconds() * refill
	if value.tokens > float64(capacity) {
		value.tokens = float64(capacity)
	}
	value.updated = now
	allowed := value.tokens >= 1
	if allowed {
		value.tokens--
	}
	m.buckets[key] = value
	return allowed, nil
}

type MemoryIdempotencyStore struct {
	mu     sync.RWMutex
	values map[string]StoredResponse
	now    func() time.Time
}

func NewMemoryIdempotencyStore(now func() time.Time) *MemoryIdempotencyStore {
	return &MemoryIdempotencyStore{values: make(map[string]StoredResponse), now: now}
}

func (m *MemoryIdempotencyStore) Get(_ context.Context, key string) (StoredResponse, bool, error) {
	m.mu.RLock()
	value, ok := m.values[key]
	m.mu.RUnlock()
	if !ok {
		return StoredResponse{}, false, nil
	}
	if !value.ExpiresAt.After(m.now()) {
		m.mu.Lock()
		delete(m.values, key)
		m.mu.Unlock()
		return StoredResponse{}, false, nil
	}
	value.Header = value.Header.Clone()
	value.Body = append([]byte(nil), value.Body...)
	return value, true, nil
}

func (m *MemoryIdempotencyStore) Put(_ context.Context, key string, value StoredResponse, ttl time.Duration) error {
	value.Header = value.Header.Clone()
	value.Body = append([]byte(nil), value.Body...)
	value.ExpiresAt = m.now().Add(ttl)
	m.mu.Lock()
	m.values[key] = value
	m.mu.Unlock()
	return nil
}
