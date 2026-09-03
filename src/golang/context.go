package oresmiddleware

import (
	"context"
	"sync"
	"time"

	nextloggers "github.com/ores-otel/ores.otel.log/sdk/go"
)

// RequestContext is the canonical ores.request-context.v1 value owned by
// ores-otel. Middleware populates it; telemetry consumes it. No second private
// context.Context key is introduced here.
type RequestContext = nextloggers.RequestContext

func WithRequestContext(parent context.Context, value RequestContext) context.Context {
	return nextloggers.WithRequestContext(parent, value)
}

func CurrentContext(ctx context.Context) (RequestContext, bool) {
	return nextloggers.RequestContextFrom(ctx)
}

func CaptureRequestContext(ctx context.Context) (RequestContext, bool) {
	return nextloggers.CaptureRequestContext(ctx)
}

func RunWithContext[T any](
	ctx context.Context,
	value RequestContext,
	operation func(context.Context) (T, error),
) (T, error) {
	return nextloggers.RunWithRequestContext(ctx, value, operation)
}

func RequestIDFrom(ctx context.Context) (string, bool) {
	return nextloggers.RequestIDFrom(ctx)
}

func LoggedInUserIDFrom(ctx context.Context) (string, bool) {
	return nextloggers.LoggedInUserIDFrom(ctx)
}

func TenantIDFrom(ctx context.Context) (string, bool) {
	return nextloggers.TenantIDFrom(ctx)
}

func SessionIDFrom(ctx context.Context) (string, bool) {
	return nextloggers.SessionIDFrom(ctx)
}

func CorrelationIDFrom(ctx context.Context) (string, bool) {
	return nextloggers.CorrelationIDFrom(ctx)
}

type registryEntry struct {
	context RequestContext
	created time.Time
}

// ContextRegistry is an optional bounded diagnostics index. It is never used
// for propagation or lookup by business logic; context.Context remains the
// source of truth. Delete each entry at request completion.
type ContextRegistry struct {
	mu         sync.RWMutex
	entries    map[string]registryEntry
	maxEntries int
	ttl        time.Duration
}

func NewContextRegistry(maxEntries int, ttl time.Duration) *ContextRegistry {
	return &ContextRegistry{
		entries:    make(map[string]registryEntry),
		maxEntries: maxEntries,
		ttl:        ttl,
	}
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
