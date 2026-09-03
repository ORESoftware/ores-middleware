package oresmiddleware

import (
	"context"
	"fmt"
	"sync"
	"testing"

	nextloggers "github.com/ores-otel/ores.otel.log/sdk/go"
)

func TestMiddlewareAndTelemetryShareOneContextValue(t *testing.T) {
	ctx := WithRequestContext(context.Background(), RequestContext{
		RequestID:      "request-go",
		LoggedInUserID: "user-go",
		TenantID:       "tenant-go",
		SessionID:      "session-go",
		CorrelationID:  "correlation-go",
		TraceID:        "0123456789abcdef0123456789abcdef",
	})

	assertContextValue(t, ctx, RequestIDFrom, "request-go")
	assertContextValue(t, ctx, LoggedInUserIDFrom, "user-go")
	assertContextValue(t, ctx, TenantIDFrom, "tenant-go")
	assertContextValue(t, ctx, SessionIDFrom, "session-go")
	assertContextValue(t, ctx, CorrelationIDFrom, "correlation-go")

	// Direct ores-otel lookup proves middleware did not introduce another key.
	assertContextValue(t, ctx, nextloggers.RequestIDFrom, "request-go")
	assertContextValue(t, ctx, nextloggers.LoggedInUserIDFrom, "user-go")
}

func TestMiddlewareRequestContextsAreIsolatedAcrossGoroutines(t *testing.T) {
	const count = 64
	var wait sync.WaitGroup
	errors := make(chan error, count)
	for index := 0; index < count; index++ {
		index := index
		wait.Add(1)
		go func() {
			defer wait.Done()
			requestID := fmt.Sprintf("request-%d", index)
			ctx := WithRequestContext(context.Background(), RequestContext{
				RequestID: requestID,
			})
			observed, ok := RequestIDFrom(ctx)
			if !ok || observed != requestID {
				errors <- fmt.Errorf("request %d observed %q, %v", index, observed, ok)
			}
		}()
	}
	wait.Wait()
	close(errors)
	for err := range errors {
		t.Error(err)
	}
}

func assertContextValue(
	t *testing.T,
	ctx context.Context,
	getter func(context.Context) (string, bool),
	want string,
) {
	t.Helper()
	got, ok := getter(ctx)
	if !ok || got != want {
		t.Fatalf("got %q, %v; want %q, true", got, ok, want)
	}
}
