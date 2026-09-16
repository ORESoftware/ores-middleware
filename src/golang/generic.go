package oresmiddleware

import (
	"context"
	"net/http"
)

// Provider is the implementation-agnostic verification port used by consumers.
// Input and output are deliberately generic so applications can keep concrete
// SDK request/response types and versions outside ores-middleware.
type Provider[Input any, Output any] interface {
	Verify(context.Context, Input) (Output, error)
}

// ProviderFunc adapts a consumer-owned closure or SDK call to Provider without
// introducing a dependency on that SDK in ores-middleware.
type ProviderFunc[Input any, Output any] func(context.Context, Input) (Output, error)

func (fn ProviderFunc[Input, Output]) Verify(ctx context.Context, input Input) (Output, error) {
	return fn(ctx, input)
}

// GenericHandler is a framework-neutral request/response handler.
type GenericHandler[Request any, Response any] func(context.Context, Request) (Response, error)

// GenericMiddleware transforms a generic handler. Consumers choose every stage
// and the exact declaration order; no ORES-specific ordering is imposed here.
type GenericMiddleware[Request any, Response any] func(GenericHandler[Request, Response]) GenericHandler[Request, Response]

// ComposeGeneric preserves declaration order: the first middleware is outermost
// and therefore runs first on the request path and last on the response path.
func ComposeGeneric[Request any, Response any](
	handler GenericHandler[Request, Response],
	middleware ...GenericMiddleware[Request, Response],
) GenericHandler[Request, Response] {
	if handler == nil {
		panic("oresmiddleware.ComposeGeneric: handler must not be nil")
	}
	return composeGeneric(handler, middleware)
}

func composeGeneric[Request any, Response any](
	handler GenericHandler[Request, Response],
	middleware []GenericMiddleware[Request, Response],
) GenericHandler[Request, Response] {
	if len(middleware) == 0 {
		return handler
	}
	stage := middleware[0]
	if stage == nil {
		panic("oresmiddleware.ComposeGeneric: middleware must not be nil")
	}
	return stage(composeGeneric(handler, middleware[1:]))
}

// AuthProviderInput is the narrow bridge from the existing net/http auth port
// to the generic provider contract. Provider implementations remain entirely
// consumer-owned.
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
