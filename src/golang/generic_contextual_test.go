package oresmiddleware

import (
	"context"
	"reflect"
	"testing"
)

func TestFallibleProviderPreservesConcreteFailureType(t *testing.T) {
	type failure struct {
		Code      string
		Retryable bool
	}
	provider := FallibleProviderFrom(func(_ context.Context, token string) ProviderResult[string, failure] {
		if token == "ok" {
			return ProviderOK[string, failure]("alice")
		}
		return ProviderFailure[string](failure{Code: "bad_token", Retryable: false})
	})

	value, gotFailure, ok := provider.Verify(context.Background(), "bad").Unpack()
	if ok {
		t.Fatal("expected typed provider failure")
	}
	if value != "" || gotFailure.Code != "bad_token" || gotFailure.Retryable {
		t.Fatalf("typed failure changed: value=%q failure=%#v", value, gotFailure)
	}
}

func TestContextualFallibleProviderPreservesAllGenericTypes(t *testing.T) {
	type request struct{ Token string }
	type metadata struct{ Tenant string }
	type decision struct{ Subject string }
	type failure struct{ Tenant string }

	provider := ContextualFallibleProviderFrom(func(
		_ context.Context,
		req request,
		meta metadata,
	) ProviderResult[decision, failure] {
		if req.Token == "ok" {
			return ProviderOK[decision, failure](decision{Subject: meta.Tenant + ":alice"})
		}
		return ProviderFailure[decision](failure{Tenant: meta.Tenant})
	})

	value, _, ok := provider.Verify(context.Background(), ContextualInput[request, metadata]{
		Request: request{Token: "ok"}, Metadata: metadata{Tenant: "t-1"},
	}).Unpack()
	if !ok || value.Subject != "t-1:alice" {
		t.Fatalf("unexpected contextual provider result: %#v ok=%v", value, ok)
	}
}

func TestComposeGenericContextualPreservesOrderAndMetadata(t *testing.T) {
	type metadata struct{ Tenant string }
	events := []string{}
	stage := func(name string) GenericContextualMiddleware[string, metadata, string] {
		return func(next GenericContextualHandler[string, metadata, string]) GenericContextualHandler[string, metadata, string] {
			return func(ctx context.Context, request string, meta metadata) (string, error) {
				events = append(events, name+":"+meta.Tenant+":before")
				response, err := next(ctx, request, meta)
				events = append(events, name+":"+meta.Tenant+":after")
				return response, err
			}
		}
	}
	handler := GenericContextualHandler[string, metadata, string](func(_ context.Context, request string, meta metadata) (string, error) {
		events = append(events, "handler:"+meta.Tenant)
		return meta.Tenant + ":" + request, nil
	})

	composed := ComposeGenericContextual(handler, stage("auth"), stage("tenant"))
	got, err := composed(context.Background(), "request", metadata{Tenant: "t-9"})
	if err != nil {
		t.Fatal(err)
	}
	if got != "t-9:request" {
		t.Fatalf("unexpected response: %q", got)
	}
	want := []string{
		"auth:t-9:before", "tenant:t-9:before", "handler:t-9",
		"tenant:t-9:after", "auth:t-9:after",
	}
	if !reflect.DeepEqual(events, want) {
		t.Fatalf("contextual order changed: got %#v want %#v", events, want)
	}
}

func TestComposeNamedGenericContextualKeepsOpaqueDuplicateNames(t *testing.T) {
	type metadata struct{ Tenant string }
	events := []string{}
	stage := func(name string) NamedGenericContextualMiddleware[string, metadata, string] {
		return NamedGenericContextualMiddleware[string, metadata, string]{
			Name: name,
			Middleware: func(next GenericContextualHandler[string, metadata, string]) GenericContextualHandler[string, metadata, string] {
				return func(ctx context.Context, request string, meta metadata) (string, error) {
					events = append(events, name+":"+meta.Tenant)
					return next(ctx, request, meta)
				}
			},
		}
	}
	handler := GenericContextualHandler[string, metadata, string](func(_ context.Context, request string, _ metadata) (string, error) {
		return request, nil
	})

	composed := ComposeNamedGenericContextual(handler, stage("auth"), stage("auth"), stage("custom-z"))
	if _, err := composed(context.Background(), "request", metadata{Tenant: "tenant-a"}); err != nil {
		t.Fatal(err)
	}
	want := []string{"auth:tenant-a", "auth:tenant-a", "custom-z:tenant-a"}
	if !reflect.DeepEqual(events, want) {
		t.Fatalf("named contextual stages changed: got %#v want %#v", events, want)
	}
}

func TestContextualEmptyChainsAreIdentityTransforms(t *testing.T) {
	type metadata struct{ Tenant string }
	handler := GenericContextualHandler[string, metadata, string](func(_ context.Context, request string, meta metadata) (string, error) {
		return meta.Tenant + ":" + request, nil
	})

	for name, composed := range map[string]GenericContextualHandler[string, metadata, string]{
		"unnamed": ComposeGenericContextual(handler),
		"named":   ComposeNamedGenericContextual(handler),
	} {
		got, err := composed(context.Background(), "request", metadata{Tenant: "t-1"})
		if err != nil || got != "t-1:request" {
			t.Fatalf("%s changed identity behavior: got=%q err=%v", name, got, err)
		}
	}
}
