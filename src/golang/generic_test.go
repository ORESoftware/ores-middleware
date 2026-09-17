package oresmiddleware

import (
	"context"
	"errors"
	"net/http/httptest"
	"reflect"
	"testing"
)

func TestGenericProviderKeepsConcreteSDKConsumerOwned(t *testing.T) {
	type consumerSDK struct {
		version string
		prefix  string
	}

	sdk := consumerSDK{version: "v7", prefix: "sdk-v7:"}
	provider := ProviderFunc[AuthProviderInput, AuthDecision](func(_ context.Context, input AuthProviderInput) (AuthDecision, error) {
		if sdk.version != "v7" {
			t.Fatalf("unexpected sdk version: %s", sdk.version)
		}
		token := input.Request.Header.Get("authorization")
		if len(token) < len(sdk.prefix) || token[:len(sdk.prefix)] != sdk.prefix {
			return AuthDecision{}, nil
		}
		return AuthDecision{UserID: token[len(sdk.prefix):]}, nil
	})

	verifier := AuthVerifierFromProvider(provider)
	request := httptest.NewRequest("GET", "https://example.test/me", nil)
	request.Header.Set("authorization", "sdk-v7:alice")
	decision, err := verifier.Verify(context.Background(), request, RequestContext{})
	if err != nil {
		t.Fatal(err)
	}
	if decision.UserID != "alice" {
		t.Fatalf("expected alice, got %q", decision.UserID)
	}
}

func TestGenericProviderPreservesConsumerErrorIdentity(t *testing.T) {
	sentinel := errors.New("consumer-provider-failure")
	provider := ProviderFrom(func(context.Context, string) (string, error) {
		return "", sentinel
	})

	_, err := provider.Verify(context.Background(), "request")
	if !errors.Is(err, sentinel) || err != sentinel {
		t.Fatalf("provider error identity changed: got %v", err)
	}
}

func TestContextualProviderIsTransportAndSDKAgnostic(t *testing.T) {
	type request struct{ Token string }
	type metadata struct{ Tenant string }
	type decision struct {
		Subject string
		Tenant  string
	}

	provider := ContextualProviderFrom(func(
		_ context.Context,
		req request,
		meta metadata,
	) (decision, error) {
		return decision{Subject: req.Token, Tenant: meta.Tenant}, nil
	})

	got, err := provider.Verify(context.Background(), ContextualInput[request, metadata]{
		Request:  request{Token: "alice"},
		Metadata: metadata{Tenant: "t-1"},
	})
	if err != nil {
		t.Fatal(err)
	}
	if got.Subject != "alice" || got.Tenant != "t-1" {
		t.Fatalf("unexpected decision: %#v", got)
	}
}

func TestComposeGenericPreservesConsumerOwnedOrdering(t *testing.T) {
	events := []string{}
	stage := func(name string) GenericMiddleware[string, string] {
		return func(next GenericHandler[string, string]) GenericHandler[string, string] {
			return func(ctx context.Context, request string) (string, error) {
				events = append(events, name+":before")
				response, err := next(ctx, request)
				events = append(events, name+":after")
				return response, err
			}
		}
	}
	handler := GenericHandler[string, string](func(_ context.Context, request string) (string, error) {
		events = append(events, "handler")
		return request + ":ok", nil
	})

	composed := ComposeGeneric(
		handler,
		stage("request-id"),
		stage("consumer-auth-v7"),
		stage("tenant-rate-limit"),
	)
	response, err := composed(context.Background(), "request")
	if err != nil {
		t.Fatal(err)
	}
	if response != "request:ok" {
		t.Fatalf("unexpected response: %q", response)
	}
	want := []string{
		"request-id:before",
		"consumer-auth-v7:before",
		"tenant-rate-limit:before",
		"handler",
		"tenant-rate-limit:after",
		"consumer-auth-v7:after",
		"request-id:after",
	}
	if !reflect.DeepEqual(events, want) {
		t.Fatalf("order mismatch\n got: %#v\nwant: %#v", events, want)
	}
}

func TestComposeNamedGenericDoesNotInterpretConsumerStageNames(t *testing.T) {
	events := []string{}
	stage := func(name string) NamedGenericMiddleware[string, string] {
		return NamedGenericMiddleware[string, string]{
			Name: name,
			Middleware: func(next GenericHandler[string, string]) GenericHandler[string, string] {
				return func(ctx context.Context, request string) (string, error) {
					events = append(events, name)
					return next(ctx, request)
				}
			},
		}
	}
	handler := GenericHandler[string, string](func(_ context.Context, request string) (string, error) {
		return request, nil
	})

	composed := ComposeNamedGeneric(
		handler,
		stage("custom-z"),
		stage("auth-provider-v42"),
		stage("custom-a"),
	)
	if _, err := composed(context.Background(), "request"); err != nil {
		t.Fatal(err)
	}
	want := []string{"custom-z", "auth-provider-v42", "custom-a"}
	if !reflect.DeepEqual(events, want) {
		t.Fatalf("stage names/order were interpreted: got %#v want %#v", events, want)
	}
}

func TestComposeNamedGenericPreservesDuplicateStages(t *testing.T) {
	events := []string{}
	stage := func(name string) NamedGenericMiddleware[string, string] {
		return NamedGenericMiddleware[string, string]{
			Name: name,
			Middleware: func(next GenericHandler[string, string]) GenericHandler[string, string] {
				return func(ctx context.Context, request string) (string, error) {
					events = append(events, name)
					return next(ctx, request)
				}
			},
		}
	}
	handler := GenericHandler[string, string](func(_ context.Context, request string) (string, error) {
		return request, nil
	})

	composed := ComposeNamedGeneric(handler, stage("auth"), stage("auth"), stage("audit"))
	if _, err := composed(context.Background(), "request"); err != nil {
		t.Fatal(err)
	}
	want := []string{"auth", "auth", "audit"}
	if !reflect.DeepEqual(events, want) {
		t.Fatalf("duplicate stages changed: got %#v want %#v", events, want)
	}
}

func TestComposeGenericRejectsNilMiddlewareAndNilResults(t *testing.T) {
	handler := GenericHandler[string, string](func(_ context.Context, request string) (string, error) {
		return request, nil
	})

	assertPanics := func(name string, fn func()) {
		t.Helper()
		t.Run(name, func(t *testing.T) {
			defer func() {
				if recover() == nil {
					t.Fatal("expected panic")
				}
			}()
			fn()
		})
	}

	assertPanics("nil middleware", func() {
		ComposeGeneric(handler, nil)
	})
	assertPanics("nil returned handler", func() {
		ComposeGeneric(handler, func(GenericHandler[string, string]) GenericHandler[string, string] {
			return nil
		})
	})
}

func TestGenericMiddlewarePropagatesConsumerContextCancellation(t *testing.T) {
	handlerRan := false
	handler := GenericHandler[string, string](func(ctx context.Context, request string) (string, error) {
		handlerRan = true
		if err := ctx.Err(); err != nil {
			return "", err
		}
		return request, nil
	})
	stage := GenericMiddleware[string, string](func(next GenericHandler[string, string]) GenericHandler[string, string] {
		return func(ctx context.Context, request string) (string, error) {
			if err := ctx.Err(); err != nil {
				return "", err
			}
			return next(ctx, request)
		}
	})
	ctx, cancel := context.WithCancel(context.Background())
	cancel()

	_, err := ComposeGeneric(handler, stage)(ctx, "request")
	if err != context.Canceled {
		t.Fatalf("expected context.Canceled, got %v", err)
	}
	if handlerRan {
		t.Fatal("cancelled context should not reach handler")
	}
}

func TestGenericEmptyChainsPreserveHandlerBehavior(t *testing.T) {
	handler := GenericHandler[string, string](func(_ context.Context, request string) (string, error) {
		return request + ":unchanged", nil
	})

	for name, composed := range map[string]GenericHandler[string, string]{
		"unnamed": ComposeGeneric(handler),
		"named":   ComposeNamedGeneric(handler),
	} {
		got, err := composed(context.Background(), "request")
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		if got != "request:unchanged" {
			t.Fatalf("%s changed handler behavior: %q", name, got)
		}
	}
}

func TestProviderConstructorsRejectNilConsumerFunctions(t *testing.T) {
	assertPanics := func(name string, fn func()) {
		t.Helper()
		t.Run(name, func(t *testing.T) {
			defer func() {
				if recover() == nil {
					t.Fatal("expected panic")
				}
			}()
			fn()
		})
	}

	assertPanics("provider", func() {
		ProviderFrom[string, string](nil)
	})
	assertPanics("contextual provider", func() {
		ContextualProviderFrom[string, int, string](nil)
	})
}
