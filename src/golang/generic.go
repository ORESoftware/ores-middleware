package oresmiddleware

import "context"

// Provider is the implementation-, transport-, framework-, and SDK-agnostic
// verification port used by Go consumers. Input and output are deliberately
// generic so applications can keep concrete SDK request/response types and
// versions outside ores-middleware.
type Provider[Input any, Output any] interface {
	Verify(context.Context, Input) (Output, error)
}

// ProviderFunc adapts a consumer-owned closure or SDK call to Provider without
// introducing a dependency on that SDK in ores-middleware.
type ProviderFunc[Input any, Output any] func(context.Context, Input) (Output, error)

func (fn ProviderFunc[Input, Output]) Verify(ctx context.Context, input Input) (Output, error) {
	return fn(ctx, input)
}

// ProviderFrom makes the consumer-owned provider boundary explicit while
// preserving concrete input/output types.
func ProviderFrom[Input any, Output any](
	verify func(context.Context, Input) (Output, error),
) Provider[Input, Output] {
	if verify == nil {
		panic("oresmiddleware.ProviderFrom: verify must not be nil")
	}
	return ProviderFunc[Input, Output](verify)
}

// ContextualInput carries a request and independently typed consumer metadata.
// It intentionally knows nothing about net/http, Gin, Echo, Fiber, TCP, NATS,
// auth SDKs, or ORES RequestContext.
type ContextualInput[Request any, Metadata any] struct {
	Request  Request
	Metadata Metadata
}

// ContextualProviderFunc adapts a request+metadata function to the generic
// Provider port without erasing either concrete type.
type ContextualProviderFunc[Request any, Metadata any, Output any] func(
	context.Context,
	Request,
	Metadata,
) (Output, error)

func (fn ContextualProviderFunc[Request, Metadata, Output]) Verify(
	ctx context.Context,
	input ContextualInput[Request, Metadata],
) (Output, error) {
	return fn(ctx, input.Request, input.Metadata)
}

func ContextualProviderFrom[Request any, Metadata any, Output any](
	verify func(context.Context, Request, Metadata) (Output, error),
) Provider[ContextualInput[Request, Metadata], Output] {
	if verify == nil {
		panic("oresmiddleware.ContextualProviderFrom: verify must not be nil")
	}
	return ContextualProviderFunc[Request, Metadata, Output](verify)
}

// GenericHandler is a framework-neutral request/response handler. Error policy
// remains consumer-owned and uses ordinary Go errors.
type GenericHandler[Request any, Response any] func(context.Context, Request) (Response, error)

// GenericMiddleware transforms a generic handler. Consumers choose every stage
// and the exact declaration order; no ORES-specific ordering is imposed here.
type GenericMiddleware[Request any, Response any] func(GenericHandler[Request, Response]) GenericHandler[Request, Response]

// NamedGenericMiddleware carries consumer-owned metadata without assigning any
// semantics to names or reordering stages.
type NamedGenericMiddleware[Request any, Response any] struct {
	Name       string
	Middleware GenericMiddleware[Request, Response]
}

// ComposeGeneric preserves declaration order: the first middleware is outermost
// and therefore runs first on the request path and last on the response path.
func ComposeGeneric[Request any, Response any](
	handler GenericHandler[Request, Response],
	middleware ...GenericMiddleware[Request, Response],
) GenericHandler[Request, Response] {
	if handler == nil {
		panic("oresmiddleware.ComposeGeneric: handler must not be nil")
	}
	for i := len(middleware) - 1; i >= 0; i-- {
		if middleware[i] == nil {
			panic("oresmiddleware.ComposeGeneric: middleware must not be nil")
		}
		handler = middleware[i](handler)
		if handler == nil {
			panic("oresmiddleware.ComposeGeneric: middleware returned nil handler")
		}
	}
	return handler
}

// ComposeNamedGeneric preserves the consumer-declared order and metadata but
// deliberately does not interpret stage names.
func ComposeNamedGeneric[Request any, Response any](
	handler GenericHandler[Request, Response],
	stages ...NamedGenericMiddleware[Request, Response],
) GenericHandler[Request, Response] {
	middleware := make([]GenericMiddleware[Request, Response], 0, len(stages))
	for _, stage := range stages {
		middleware = append(middleware, stage.Middleware)
	}
	return ComposeGeneric(handler, middleware...)
}
