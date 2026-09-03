package oresmiddleware

import (
	"context"
	"errors"
	"log/slog"
	"net/http"
	"strings"

	nextloggers "github.com/ores-otel/ores.otel.log/sdk/go"
)

// Re-export the canonical ores-otel Go contracts so applications can depend on
// ores-middleware as their single middleware/logging integration surface.
type OresLogger = nextloggers.Logger
type OresLogEvent = nextloggers.Event
type OresLogContext = nextloggers.LogContext
type OresLoggerOptions = nextloggers.Options
type OresLogLevel = nextloggers.Level

var NewOresLogger = nextloggers.NewLogger

var ErrOresLoggerUnavailable = errors.New("ores middleware: request logger unavailable")

type oresLoggerContextKey struct{}

// WithOresLogger installs a request logger handle next to the portable,
// serializable RequestContext. Logger objects never become part of that wire
// contract.
func WithOresLogger(parent context.Context, logger *nextloggers.Logger) context.Context {
	if parent == nil {
		parent = context.Background()
	}
	if logger == nil {
		return parent
	}
	return context.WithValue(parent, oresLoggerContextKey{}, logger)
}

// OresLoggerFromContext returns the request-specific child logger created by
// WrapWithOresLogger.
func OresLoggerFromContext(ctx context.Context) (*nextloggers.Logger, bool) {
	if ctx == nil {
		return nil, false
	}
	logger, ok := ctx.Value(oresLoggerContextKey{}).(*nextloggers.Logger)
	return logger, ok && logger != nil
}

// ToOresLogContext maps the data-only middleware context onto the canonical
// ores-otel context. Only allow-listed correlation metadata is propagated.
func ToOresLogContext(value RequestContext) nextloggers.LogContext {
	fields := map[string]any{
		"request.id":                 value.RequestID,
		"trace.id":                   value.TraceID,
		"request.started_at_unix_ms": value.StartedAtUnixMS,
	}
	if value.UserID != "" {
		fields["user.id"] = value.UserID
	}
	if value.TenantID != "" {
		fields["tenant.id"] = value.TenantID
	}
	if value.Locale != "" {
		fields["request.locale"] = value.Locale
	}
	if value.DeadlineUnixMS > 0 {
		fields["request.deadline_unix_ms"] = value.DeadlineUnixMS
	}

	baggage := make(map[string]string)
	for key, item := range value.Baggage {
		if strings.HasPrefix(key, "otel.") {
			baggage[key] = item
		}
	}

	var user map[string]any
	if value.UserID != "" {
		user = map[string]any{"id": value.UserID}
	}
	var traceIDs []string
	if value.TraceID != "" {
		traceIDs = []string{value.TraceID}
	}

	return nextloggers.LogContext{
		LoggedInUser: user,
		Fields:       fields,
		TraceID:      value.TraceID,
		TraceIDs:     traceIDs,
		SpanID:       value.SpanID,
		Baggage:      baggage,
		RoutineID:    value.RequestID,
		Tags:         []string{"ores-middleware", "request"},
	}
}

func cloneOresFields(source map[string]any) map[string]any {
	target := make(map[string]any, len(source))
	for key, value := range source {
		target[key] = value
	}
	return target
}

// CreateOresRequestLogger derives a logger per request while retaining the
// root logger's transports, level, lifecycle hooks, and configured file fields.
func CreateOresRequestLogger(root *nextloggers.Logger, value RequestContext) *nextloggers.Logger {
	if root == nil {
		return nil
	}
	logContext := ToOresLogContext(value)
	fields := cloneOresFields(root.Fields)
	for key, item := range logContext.Fields {
		fields[key] = item
	}
	user := cloneOresFields(root.CurrentUser)
	for key, item := range logContext.LoggedInUser {
		user[key] = item
	}
	name := "request"
	if root.Name != "" {
		name = root.Name + ":request"
	}
	otelEnabled := root.OtelEnabled
	child := nextloggers.NewLogger(nextloggers.Options{
		AppName:      root.AppName,
		Name:         name,
		Runtime:      root.Runtime,
		MaxLevel:     root.MaxLevel,
		Fields:       fields,
		LoggedInUser: user,
		Transports:   append([]nextloggers.Transport(nil), root.Transports...),
		Console:      root.Console,
		OtelEnabled:  &otelEnabled,
		Output:       root.Output,
		IDFactory:    root.IDFactory,
		Clock:        root.Clock,
	})
	child.RuntimeFields = root.RuntimeFields
	return child
}

// RequestLog provides handler-friendly `Log(ctx).Info/Warn/Error` calls. Go's
// context.Context remains the source of truth across goroutines and deadlines.
type RequestLog struct {
	ctx    context.Context
	logger *nextloggers.Logger
}

func Log(ctx context.Context) RequestLog {
	logger, _ := OresLoggerFromContext(ctx)
	return RequestLog{ctx: ctx, logger: logger}
}

func (log RequestLog) Logger() (*nextloggers.Logger, bool) {
	return log.logger, log.logger != nil
}

func (log RequestLog) Trace(values ...any) error {
	if log.logger == nil {
		return ErrOresLoggerUnavailable
	}
	return log.logger.TraceContext(log.ctx, values...).Send()
}

func (log RequestLog) Debug(values ...any) error {
	if log.logger == nil {
		return ErrOresLoggerUnavailable
	}
	return log.logger.DebugContext(log.ctx, values...).Send()
}

func (log RequestLog) Info(values ...any) error {
	if log.logger == nil {
		return ErrOresLoggerUnavailable
	}
	return log.logger.InfoContext(log.ctx, values...).Send()
}

func (log RequestLog) Warn(values ...any) error {
	if log.logger == nil {
		return ErrOresLoggerUnavailable
	}
	return log.logger.WarnContext(log.ctx, values...).Send()
}

func (log RequestLog) Error(values ...any) error {
	if log.logger == nil {
		return ErrOresLoggerUnavailable
	}
	return log.logger.ErrorContext(log.ctx, values...).Send()
}

func (log RequestLog) Fatal(values ...any) error {
	if log.logger == nil {
		return ErrOresLoggerUnavailable
	}
	return log.logger.FatalContext(log.ctx, values...).Send()
}

func emitOresRequestLog(ctx context.Context, event *nextloggers.Event, phase string) {
	if err := event.Send(); err != nil {
		slog.WarnContext(ctx, "ores request log failed", "phase", phase, "error", err)
	}
}

// WrapWithOresLogger composes the portable stack with ores-otel. It runs after
// authentication, creates a child pinned to request/user/tenant identifiers,
// and places both the child and immutable log context on request.Context().
func (s *Stack) WrapWithOresLogger(root *nextloggers.Logger, next http.Handler) http.Handler {
	if root == nil {
		return s.Wrap(next)
	}

	return s.Wrap(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		value, ok := CurrentContext(request.Context())
		if !ok {
			next.ServeHTTP(writer, request)
			return
		}

		logger := CreateOresRequestLogger(root, value)
		ctx := nextloggers.WithLogContext(request.Context(), ToOresLogContext(value))
		ctx = WithOresLogger(ctx, logger)
		request = request.WithContext(ctx)
		fields := map[string]any{
			"http.request.method": request.Method,
			"url.path":            request.URL.Path,
		}

		emitOresRequestLog(ctx, logger.InfoContext(ctx, "request handler started").AddFields(fields), "started")
		defer func() {
			if recovered := recover(); recovered != nil {
				emitOresRequestLog(
					ctx,
					logger.ErrorContext(ctx, "request handler panic").AddFields(map[string]any{
						"http.request.method": request.Method,
						"url.path":            request.URL.Path,
						"request.outcome":     "panic",
					}),
					"panic",
				)
				panic(recovered)
			}

			switch {
			case errors.Is(ctx.Err(), context.DeadlineExceeded):
				emitOresRequestLog(
					ctx,
					logger.ErrorContext(ctx, "request handler timed out").AddFields(map[string]any{
						"http.request.method":       request.Method,
						"url.path":                  request.URL.Path,
						"http.response.status_code": http.StatusGatewayTimeout,
						"request.outcome":           "timeout",
					}),
					"timeout",
				)
			case errors.Is(ctx.Err(), context.Canceled):
				emitOresRequestLog(
					ctx,
					logger.WarnContext(ctx, "request handler cancelled").AddFields(map[string]any{
						"http.request.method": request.Method,
						"url.path":            request.URL.Path,
						"request.outcome":     "cancelled",
					}),
					"cancelled",
				)
			default:
				emitOresRequestLog(
					ctx,
					logger.InfoContext(ctx, "request handler completed").AddFields(map[string]any{
						"http.request.method": request.Method,
						"url.path":            request.URL.Path,
						"request.outcome":     "completed",
					}),
					"completed",
				)
			}
		}()

		next.ServeHTTP(writer, request)
	}))
}
