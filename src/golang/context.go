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

func cloneRequestContext(value RequestContext) RequestContext {
	if value.Baggage == nil {
		return value
	}
	baggage := make(map[string]string, len(value.Baggage))
	for key, item := range value.Baggage {
		baggage[key] = item
	}
	value.Baggage = baggage
	return value
}

// WithRequestContext stores one aggregate value behind one unexported key. The
// baggage map is copied so sibling goroutines cannot race by mutating a shared
// map after the context has been installed.
func WithRequestContext(parent context.Context, value RequestContext) context.Context {
	if parent == nil {
		parent = context.Background()
	}
	return context.WithValue(parent, requestContextKey{}, cloneRequestContext(value))
}

// CurrentContext returns a defensive copy of the request context.
func CurrentContext(ctx context.Context) (RequestContext, bool) {
	if ctx == nil {
		return RequestContext{}, false
	}
	value, ok := ctx.Value(requestContextKey{}).(RequestContext)
	if !ok {
		return RequestContext{}, false
	}
	return cloneRequestContext(value), true
}

func RunWithContext[T any](ctx context.Context, value RequestContext, operation func(context.Context) (T, error)) (T, error) {
	return operation(WithRequestContext(ctx, value))
}

// RequestIDFromContext performs one context lookup and returns the request ID.
func RequestIDFromContext(ctx context.Context) (string, bool) {
	value, ok := CurrentContext(ctx)
	return value.RequestID, ok && value.RequestID != ""
}

// TraceIDFromContext performs one context lookup and returns the W3C trace ID.
func TraceIDFromContext(ctx context.Context) (string, bool) {
	value, ok := CurrentContext(ctx)
	return value.TraceID, ok && value.TraceID != ""
}

// UserIDFromContext returns the authenticated user ID, when one was established.
func UserIDFromContext(ctx context.Context) (string, bool) {
	value, ok := CurrentContext(ctx)
	return value.UserID, ok && value.UserID != ""
}

// LoggedInUserIDFromContext is an explicit naming alias for application code.
func LoggedInUserIDFromContext(ctx context.Context) (string, bool) {
	return UserIDFromContext(ctx)
}

// TenantIDFromContext returns the authenticated tenant ID, when present.
func TenantIDFromContext(ctx context.Context) (string, bool) {
	value, ok := CurrentContext(ctx)
	return value.TenantID, ok && value.TenantID != ""
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
	r.entries[value.RequestID] = registryEntry{context: cloneRequestContext(value), created: now}
}

func (r *ContextRegistry) Get(requestID string) (RequestContext, bool) {
	r.mu.RLock()
	entry, ok := r.entries[requestID]
	r.mu.RUnlock()
	if !ok || time.Since(entry.created) > r.ttl {
		return RequestContext{}, false
	}
	return cloneRequestContext(entry.context), true
}

func (r *ContextRegistry) Delete(requestID string) {
	r.mu.Lock()
	delete(r.entries, requestID)
	r.mu.Unlock()
}
