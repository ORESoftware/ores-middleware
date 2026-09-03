package oresmiddleware

import (
	"context"
	"errors"
	"log/slog"
	"reflect"
)

type OperationTransport string

const (
	OperationTransportHTTP      OperationTransport = "http"
	OperationTransportTCP       OperationTransport = "tcp"
	OperationTransportWebSocket OperationTransport = "websocket"
)

type OperationScope string

const (
	OperationScopeRequest    OperationScope = "request"
	OperationScopeConnection OperationScope = "connection"
	OperationScopeMessage    OperationScope = "message"
	OperationScopeCallback   OperationScope = "callback"
)

type OperationFailureKind string

const (
	OperationFailureError            OperationFailureKind = "error"
	OperationFailurePanic            OperationFailureKind = "panic"
	OperationFailureCancelled        OperationFailureKind = "cancelled"
	OperationFailureDeadlineExceeded OperationFailureKind = "deadline_exceeded"
)

type OperationDescriptor struct {
	Transport OperationTransport
	Scope     OperationScope
	Name      string
}

// OperationFailure contains only bounded classification and correlation data.
// The panic value or returned error is never copied into this public value.
type OperationFailure struct {
	Kind      OperationFailureKind
	Code      string
	Transport OperationTransport
	Scope     OperationScope
	Operation string
	RequestID string
	TraceID   string
	ErrorType string
}

type OperationOutcome[T any] struct {
	Value   T
	Failure *OperationFailure
}

func (outcome OperationOutcome[T]) OK() bool { return outcome.Failure == nil }

type OperationFailureReporter func(context.Context, OperationFailure)

// CaptureOperationContext takes a defensive copy suitable for an explicitly
// propagated goroutine, TCP connection, WebSocket message, or queue callback.
func CaptureOperationContext(ctx context.Context) (RequestContext, bool) {
	return CurrentContext(ctx)
}

// RunOperationBoundary turns one callback into an isolated failure domain.
// Panics and returned errors become typed outcomes. Reporter panics are ignored
// so observability cannot replace the application/protocol outcome.
func RunOperationBoundary[T any](
	parent context.Context,
	requestContext RequestContext,
	descriptor OperationDescriptor,
	reporter OperationFailureReporter,
	operation func(context.Context) (T, error),
) (outcome OperationOutcome[T]) {
	if parent == nil {
		parent = context.Background()
	}
	ctx := WithRequestContext(parent, requestContext)

	defer func() {
		if recovered := recover(); recovered != nil {
			failure := newOperationFailure(
				OperationFailurePanic,
				descriptor,
				requestContext,
				safeErrorType(recovered, "panic"),
			)
			outcome = OperationOutcome[T]{Failure: &failure}
			reportOperationFailure(ctx, reporter, failure)
		}
	}()

	if failure, failed := contextFailure(ctx, descriptor, requestContext); failed {
		reportOperationFailure(ctx, reporter, failure)
		return OperationOutcome[T]{Failure: &failure}
	}

	value, err := operation(ctx)
	if err != nil {
		kind := OperationFailureError
		if errors.Is(err, context.DeadlineExceeded) {
			kind = OperationFailureDeadlineExceeded
		} else if errors.Is(err, context.Canceled) {
			kind = OperationFailureCancelled
		}
		failure := newOperationFailure(kind, descriptor, requestContext, safeErrorType(err, "error"))
		reportOperationFailure(ctx, reporter, failure)
		return OperationOutcome[T]{Failure: &failure}
	}

	if failure, failed := contextFailure(ctx, descriptor, requestContext); failed {
		reportOperationFailure(ctx, reporter, failure)
		return OperationOutcome[T]{Failure: &failure}
	}
	return OperationOutcome[T]{Value: value}
}

func contextFailure(
	ctx context.Context,
	descriptor OperationDescriptor,
	requestContext RequestContext,
) (OperationFailure, bool) {
	switch {
	case errors.Is(ctx.Err(), context.DeadlineExceeded):
		return newOperationFailure(OperationFailureDeadlineExceeded, descriptor, requestContext, "DeadlineExceeded"), true
	case errors.Is(ctx.Err(), context.Canceled):
		return newOperationFailure(OperationFailureCancelled, descriptor, requestContext, "Canceled"), true
	default:
		return OperationFailure{}, false
	}
}

func newOperationFailure(
	kind OperationFailureKind,
	descriptor OperationDescriptor,
	requestContext RequestContext,
	errorType string,
) OperationFailure {
	code := "operation_failed"
	switch kind {
	case OperationFailureDeadlineExceeded:
		code = "operation_deadline_exceeded"
	case OperationFailureCancelled:
		code = "operation_cancelled"
	case OperationFailurePanic:
		code = "operation_panicked"
	}
	return OperationFailure{
		Kind:      kind,
		Code:      code,
		Transport: descriptor.Transport,
		Scope:     descriptor.Scope,
		Operation: safeOperationName(descriptor.Name),
		RequestID: requestContext.RequestID,
		TraceID:   requestContext.TraceID,
		ErrorType: errorType,
	}
}

func safeOperationName(value string) string {
	if len(value) == 0 || len(value) > 128 {
		return "operation"
	}
	for _, item := range value {
		if !((item >= 'a' && item <= 'z') || (item >= 'A' && item <= 'Z') || (item >= '0' && item <= '9') || item == '_' || item == '-' || item == '.' || item == ':') {
			return "operation"
		}
	}
	return value
}

func safeErrorType(value any, fallback string) string {
	valueType := reflect.TypeOf(value)
	if valueType == nil {
		return fallback
	}
	for valueType.Kind() == reflect.Pointer {
		valueType = valueType.Elem()
	}
	name := valueType.Name()
	if name == "" || len(name) > 64 {
		return fallback
	}
	for _, item := range name {
		if !((item >= 'a' && item <= 'z') || (item >= 'A' && item <= 'Z') || (item >= '0' && item <= '9') || item == '_' || item == '-' || item == '.') {
			return fallback
		}
	}
	return name
}

func reportOperationFailure(
	ctx context.Context,
	reporter OperationFailureReporter,
	failure OperationFailure,
) {
	defer func() { _ = recover() }()
	if reporter != nil {
		reporter(ctx, failure)
		return
	}
	slog.ErrorContext(
		ctx,
		"operation failed",
		"operation.name", failure.Operation,
		"operation.transport", failure.Transport,
		"operation.scope", failure.Scope,
		"operation.outcome", failure.Kind,
		"error.type", failure.ErrorType,
		"request_id", failure.RequestID,
		"trace_id", failure.TraceID,
	)
}

// OresOperationFailureReporter emits a failure through the request logger
// installed by WrapWithOresLogger. Missing or failed transports remain fail-open.
func OresOperationFailureReporter(ctx context.Context, failure OperationFailure) {
	_ = Log(ctx).Error(
		"operation failed",
		map[string]any{
			"operation.name":      failure.Operation,
			"operation.transport": failure.Transport,
			"operation.scope":     failure.Scope,
			"operation.outcome":   failure.Kind,
			"error.type":          failure.ErrorType,
			"request.id":          failure.RequestID,
			"trace.id":            failure.TraceID,
		},
	)
}
