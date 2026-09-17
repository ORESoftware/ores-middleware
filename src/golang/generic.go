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

// ProviderResult is a typed, provider-neutral success/failure value for
// consumers that do not want to collapse domain failures into Go's error
// interface. The concrete Failure type remains consumer-owned.
type ProviderResult[Output any, Failure any] struct {
	value   Output
	failure Failure
	ok      bool
}

// ProviderOK constructs a successful typed provider result.
func ProviderOK[Output any, Failure any](value Output) ProviderResult[Output, Failure] {
	return ProviderResult[Output, Failure]{value: value, ok: true}
}

// ProviderFailure constructs a failed typed provider result without converting
// the consumer's failure value into a provider-specific or string error.
func ProviderFailure[Output any, Failure any](failure Failure) ProviderResult[Output, Failure] {
	return ProviderResult[Output, Failure]{failure: failure}
}

// Unpack returns the success value, failure value, and success discriminator.
// Exactly one of value/failure is semantically active according to ok.
func (result ProviderResult[Output, Failure]) Unpack() (value Output, failure Failure, ok bool) {
	return result.value, result.failure, result.ok
}

// FallibleProvider is the typed-failure counterpart to Provider. Domain failure
// semantics remain fully generic; context cancellation and transport errors can
// still be modeled separately by using Provider when that is more idiomatic.
type FallibleProvider[Input any, Output any, Failure any] interface {
	Verify(context.Context, Input) ProviderResult[Output, Failure]
}

// FallibleProviderFunc adapts a consumer-owned function to FallibleProvider.
type FallibleProviderFunc[Input any, Output any, Failure any] func(
	context.Context,
	Input,
) ProviderResult[Output, Failure]

func (fn FallibleProviderFunc[Input, Output, Failure]) Verify(
	ctx context.Context,
	input Input,
) ProviderResult[Output, Failure] {
	return fn(ctx, input)
}

func FallibleProviderFrom[Input any, Output any, Failure any](
	verify func(context.Context, Input) ProviderResult[Output, Failure],
) FallibleProvider[Input, Output, Failure] {
	if verify == nil {
		panic("oresmiddleware.FallibleProviderFrom: verify must not be nil")
	}
	return FallibleProviderFunc[Input, Output, Failure](verify)
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

// ContextualFallibleProviderFunc is the typed-failure contextual provider
// adapter. Request, metadata, output, and failure all remain concrete generic
// parameters owned by the consumer.
type ContextualFallibleProviderFunc[Request any, Metadata any, Output any, Failure any] func(
	context.Context,
	Request,
	Metadata,
) ProviderResult[Output, Failure]

func (fn ContextualFallibleProviderFunc[Request, Metadata, Output, Failure]) Verify(
	ctx context.Context,
	input ContextualInput[Request, Metadata],
) ProviderResult[Output, Failure] {
	return fn(ctx, input.Request, input.Metadata)
}

func ContextualFallibleProviderFrom[Request any, Metadata any, Output any, Failure any](
	verify func(context.Context, Request, Metadata) ProviderResult[Output, Failure],
) FallibleProvider[ContextualInput[Request, Metadata], Output, Failure] {
	if verify == nil {
		panic("oresmiddleware.ContextualFallibleProviderFrom: verify must not be nil")
	}
	return ContextualFallibleProviderFunc[Request, Metadata, Output, Failure](verify)
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

// GenericContextualHandler is the request+metadata counterpart to
// GenericHandler. Metadata is passed explicitly instead of relying on package
// globals, framework locals, or one ORES context representation.
type GenericContextualHandler[Request any, Metadata any, Response any] func(
	context.Context,
	Request,
	Metadata,
) (Response, error)

// GenericContextualMiddleware transforms a contextual generic handler while
// preserving the consumer's concrete metadata type and declaration order.
type GenericContextualMiddleware[Request any, Metadata any, Response any] func(
	GenericContextualHandler[Request, Metadata, Response],
) GenericContextualHandler[Request, Metadata, Response]

// NamedGenericContextualMiddleware carries opaque consumer-owned stage names.
type NamedGenericContextualMiddleware[Request any, Metadata any, Response any] struct {
	Name       string
	Middleware GenericContextualMiddleware[Request, Metadata, Response]
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

// ComposeGenericContextual preserves declaration order for contextual
// middleware and keeps metadata explicit end-to-end.
func ComposeGenericContextual[Request any, Metadata any, Response any](
	handler GenericContextualHandler[Request, Metadata, Response],
	middleware ...GenericContextualMiddleware[Request, Metadata, Response],
) GenericContextualHandler[Request, Metadata, Response] {
	if handler == nil {
		panic("oresmiddleware.ComposeGenericContextual: handler must not be nil")
	}
	for i := len(middleware) - 1; i >= 0; i-- {
		if middleware[i] == nil {
			panic("oresmiddleware.ComposeGenericContextual: middleware must not be nil")
		}
		handler = middleware[i](handler)
		if handler == nil {
			panic("oresmiddleware.ComposeGenericContextual: middleware returned nil handler")
		}
	}
	return handler
}

// ComposeNamedGenericContextual is the named contextual counterpart. Names are
// carried for the consumer but never sorted, deduplicated, or interpreted.
func ComposeNamedGenericContextual[Request any, Metadata any, Response any](
	handler GenericContextualHandler[Request, Metadata, Response],
	stages ...NamedGenericContextualMiddleware[Request, Metadata, Response],
) GenericContextualHandler[Request, Metadata, Response] {
	middleware := make([]GenericContextualMiddleware[Request, Metadata, Response], 0, len(stages))
	for _, stage := range stages {
		middleware = append(middleware, stage.Middleware)
	}
	return ComposeGenericContextual(handler, middleware...)
}
