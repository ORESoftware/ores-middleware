package oresmiddleware

import (
	"context"
	"net/http"
)

// AuthProviderInput is the narrow net/http bridge from the framework-agnostic
// Provider contract to the existing AuthVerifier convenience surface.
// Provider implementations remain entirely consumer-owned.
type AuthProviderInput struct {
	Request *http.Request
	Context RequestContext
}

type authProviderAdapter[P Provider[AuthProviderInput, AuthDecision]] struct {
	provider P
}

func (adapter authProviderAdapter[P]) Verify(
	ctx context.Context,
	request *http.Request,
	requestContext RequestContext,
) (AuthDecision, error) {
	return adapter.provider.Verify(ctx, AuthProviderInput{
		Request: request,
		Context: requestContext,
	})
}

// AuthVerifierFromProvider adapts any generic provider to the legacy/convenience
// AuthVerifier surface without type-erasing or importing its concrete SDK.
func AuthVerifierFromProvider[P Provider[AuthProviderInput, AuthDecision]](provider P) AuthVerifier {
	return authProviderAdapter[P]{provider: provider}
}
