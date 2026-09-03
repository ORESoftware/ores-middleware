package oresmiddleware

import (
	"context"
	"testing"
	"time"
)

func TestTypedContextAccessorsAndDefensiveCopies(t *testing.T) {
	original := RequestContext{
		RequestID: "request-42",
		TraceID:   "0123456789abcdef0123456789abcdef",
		UserID:    "user-42",
		TenantID:  "tenant-7",
		Baggage:   map[string]string{"otel.vendor": "original"},
	}
	ctx := WithRequestContext(context.Background(), original)

	// Mutating the caller-owned map after installation must not affect readers.
	original.Baggage["otel.vendor"] = "caller-mutated"
	stored, ok := CurrentContext(ctx)
	if !ok {
		t.Fatal("request context not found")
	}
	if got := stored.Baggage["otel.vendor"]; got != "original" {
		t.Fatalf("installed baggage mutated through caller map: %q", got)
	}

	// Mutating one retrieved copy must not affect a later lookup either.
	stored.Baggage["otel.vendor"] = "reader-mutated"
	again, ok := CurrentContext(ctx)
	if !ok || again.Baggage["otel.vendor"] != "original" {
		t.Fatalf("context lookup did not return a defensive copy: %#v", again.Baggage)
	}

	assertContextValue(t, "request ID", RequestIDFromContext, ctx, "request-42")
	assertContextValue(t, "trace ID", TraceIDFromContext, ctx, "0123456789abcdef0123456789abcdef")
	assertContextValue(t, "user ID", UserIDFromContext, ctx, "user-42")
	assertContextValue(t, "logged-in user ID", LoggedInUserIDFromContext, ctx, "user-42")
	assertContextValue(t, "tenant ID", TenantIDFromContext, ctx, "tenant-7")
}

func TestContextRegistryDoesNotExposeMutableBaggage(t *testing.T) {
	registry := NewContextRegistry(4, time.Minute)
	value := RequestContext{
		RequestID: "registry-request",
		Baggage:   map[string]string{"otel.slot": "one"},
	}
	registry.Put(value)
	value.Baggage["otel.slot"] = "caller-mutated"

	first, ok := registry.Get("registry-request")
	if !ok || first.Baggage["otel.slot"] != "one" {
		t.Fatalf("registry retained caller-owned baggage: %#v", first.Baggage)
	}
	first.Baggage["otel.slot"] = "reader-mutated"
	second, ok := registry.Get("registry-request")
	if !ok || second.Baggage["otel.slot"] != "one" {
		t.Fatalf("registry exposed internal baggage map: %#v", second.Baggage)
	}
}

func TestContextAccessorsOutsideRequest(t *testing.T) {
	for name, accessor := range map[string]func(context.Context) (string, bool){
		"request": RequestIDFromContext,
		"trace":   TraceIDFromContext,
		"user":    UserIDFromContext,
		"logged":  LoggedInUserIDFromContext,
		"tenant":  TenantIDFromContext,
	} {
		if value, ok := accessor(context.Background()); ok || value != "" {
			t.Fatalf("%s accessor unexpectedly found %q", name, value)
		}
		if value, ok := accessor(nil); ok || value != "" {
			t.Fatalf("%s accessor unexpectedly accepted nil context: %q", name, value)
		}
	}
}

func assertContextValue(
	t *testing.T,
	name string,
	accessor func(context.Context) (string, bool),
	ctx context.Context,
	want string,
) {
	t.Helper()
	got, ok := accessor(ctx)
	if !ok || got != want {
		t.Fatalf("%s = %q, %v; want %q, true", name, got, ok, want)
	}
}
