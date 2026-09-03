package oresmiddleware

import (
	"context"
	"sync"
	"time"
)

type RequestContext struct {
	RequestID       string            `json:"requestId"`
	TraceID         string            `json:"traceId"`
	SpanID          string            `json:"spanId,omitempty"`
	TenantID        string            `json:"tenantId,omitempty"`
	UserID          string            `json:"userId,omitempty"`
	Locale          string            `json:"locale,omitempty"`
	StartedAtUnixMS int64             `json:"startedAtUnixMs"`
	DeadlineUnixMS  int64             `json:"deadlineUnixMs,omitempty"`
	Baggage         map[string]string `json:"baggage"`
}

type requestContextKey struct{}

func WithRequestContext(parent context.Context, value RequestContext) context.Context {
	return context.WithValue(parent, requestContextKey{}, value)
}

func CurrentContext(ctx context.Context) (RequestContext, bool) {
	value, ok := ctx.Value(requestContextKey{}).(RequestContext)
	return value, ok
}

func RunWithContext[T any](ctx context.Context, value RequestContext, operation func(context.Context) (T, error)) (T, error) {
	return operation(WithRequestContext(ctx, value))
}

type registryEntry struct {
	context RequestContext
	created time.Time
}

type ContextRegistry struct {
	mu         sync.RWMutex
	entries    map[string]registryEntry
	maxEntries int
	ttl        time.Duration
}

func NewContextRegistry(maxEntries int, ttl time.Duration) *ContextRegistry {
	return &ContextRegistry{entries: make(map[string]registryEntry), maxEntries: maxEntries, ttl: ttl}
}

func (r *ContextRegistry) Put(value RequestContext) {
	if r.maxEntries <= 0 {
		return
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	now := time.Now()
	for key, entry := range r.entries {
		if now.Sub(entry.created) > r.ttl {
			delete(r.entries, key)
		}
	}
	if len(r.entries) >= r.maxEntries {
		var oldestKey string
		var oldest time.Time
		for key, entry := range r.entries {
			if oldest.IsZero() || entry.created.Before(oldest) {
				oldest, oldestKey = entry.created, key
			}
		}
		delete(r.entries, oldestKey)
	}
	r.entries[value.RequestID] = registryEntry{context: value, created: now}
}

func (r *ContextRegistry) Get(requestID string) (RequestContext, bool) {
	r.mu.RLock()
	entry, ok := r.entries[requestID]
	r.mu.RUnlock()
	if !ok || time.Since(entry.created) > r.ttl {
		return RequestContext{}, false
	}
	return entry.context, true
}

func (r *ContextRegistry) Delete(requestID string) {
	r.mu.Lock()
	delete(r.entries, requestID)
	r.mu.Unlock()
}
