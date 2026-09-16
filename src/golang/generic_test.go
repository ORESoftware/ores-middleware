package oresmiddleware

import (
	"context"
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
