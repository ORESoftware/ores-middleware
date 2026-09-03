from pathlib import Path


def replace_once(path: str, before: str, after: str) -> None:
    target = Path(path)
    text = target.read_text(encoding="utf-8")
    count = text.count(before)
    if count != 1:
        raise RuntimeError(f"expected exactly one match in {path}, found {count}: {before[:120]!r}")
    target.write_text(text.replace(before, after, 1), encoding="utf-8")


replace_once(
    "src/golang/middleware.go",
    '''\t"slices"\n\t"strings"\n\t"time"''',
    '''\t"slices"\n\t"strings"\n\t"sync"\n\t"time"''',
)

replace_once(
    "src/golang/middleware.go",
    '''func (s *Stack) Wrap(next http.Handler) http.Handler {\n\treturn http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {\n\t\tstarted := s.deps.Now()\n\t\tif request.ContentLength > s.config.Settings.MaxBodyBytes {''',
    '''func (s *Stack) Wrap(next http.Handler) http.Handler {\n\treturn http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {\n\t\tstarted := s.deps.Now()\n\t\trequestID := validToken(request.Header.Get(s.config.Settings.RequestIDHeader))\n\t\tif requestID == "" {\n\t\t\trequestID = randomHex(16)\n\t\t}\n\t\ttraceID := parseTraceID(request.Header.Get(s.config.Settings.TraceHeader))\n\t\tif traceID == "" {\n\t\t\ttraceID = randomHex(16)\n\t\t}\n\t\tvalue := RequestContext{RequestID: requestID, TraceID: traceID, Locale: request.Header.Get("Accept-Language"), StartedAtUnixMS: started.UnixMilli(), DeadlineUnixMS: started.Add(time.Duration(s.config.Settings.TimeoutMS) * time.Millisecond).UnixMilli(), Baggage: map[string]string{}}\n\t\tctx, cancel := context.WithTimeout(request.Context(), time.Duration(s.config.Settings.TimeoutMS)*time.Millisecond)\n\t\tdefer cancel()\n\t\tctx = WithRequestContext(ctx, value)\n\t\tctx = WithOresLogContext(ctx, value)\n\t\trequest = request.WithContext(ctx)\n\n\t\tresponseCommitted := false\n\t\tdefer func() {\n\t\t\tif recovered := recover(); recovered != nil {\n\t\t\t\tfailure := newOperationFailure(\n\t\t\t\t\tOperationFailurePanic,\n\t\t\t\t\tOperationDescriptor{Transport: OperationTransportHTTP, Scope: OperationScopeRequest, Name: "middleware.http"},\n\t\t\t\t\tvalue,\n\t\t\t\t\tsafeErrorType(recovered, "panic"),\n\t\t\t\t)\n\t\t\t\treportOperationFailure(ctx, s.deps.OperationFailureReporter, failure)\n\t\t\t\tif responseCommitted {\n\t\t\t\t\treturn\n\t\t\t\t}\n\t\t\t\tcapture := problemResponse(http.StatusInternalServerError, "internal_error", "request processing failed")\n\t\t\t\tstatus, headers, body := capture.snapshot()\n\t\t\t\tapplySecurityHeaders(s.config, headers)\n\t\t\t\theaders.Set(s.config.Settings.RequestIDHeader, value.RequestID)\n\t\t\t\tresponseCommitted = true\n\t\t\t\tcopyResponse(writer, status, headers, body)\n\t\t\t}\n\t\t}()\n\n\t\tif request.ContentLength > s.config.Settings.MaxBodyBytes {''',
)

replace_once(
    "src/golang/middleware.go",
    '''\n\t\trequestID := validToken(request.Header.Get(s.config.Settings.RequestIDHeader))\n\t\tif requestID == "" {\n\t\t\trequestID = randomHex(16)\n\t\t}\n\t\ttraceID := parseTraceID(request.Header.Get(s.config.Settings.TraceHeader))\n\t\tif traceID == "" {\n\t\t\ttraceID = randomHex(16)\n\t\t}\n\t\tvalue := RequestContext{RequestID: requestID, TraceID: traceID, Locale: request.Header.Get("Accept-Language"), StartedAtUnixMS: started.UnixMilli(), DeadlineUnixMS: started.Add(time.Duration(s.config.Settings.TimeoutMS) * time.Millisecond).UnixMilli(), Baggage: map[string]string{}}\n\t\tctx, cancel := context.WithTimeout(request.Context(), time.Duration(s.config.Settings.TimeoutMS)*time.Millisecond)\n\t\tdefer cancel()\n\t\tctx = WithRequestContext(ctx, value)\n\t\trequest = request.WithContext(ctx)\n''',
    '''\n''',
)

replace_once(
    "src/golang/middleware.go",
    '''\t\tctx = WithRequestContext(ctx, value)\n\t\trequest = request.WithContext(ctx)\n\t\tif s.config.Integrations.SharedAuth.Mode != IntegrationDisabled''',
    '''\t\tctx = WithRequestContext(ctx, value)\n\t\tctx = WithOresLogContext(ctx, value)\n\t\trequest = request.WithContext(ctx)\n\t\tif s.config.Integrations.SharedAuth.Mode != IntegrationDisabled''',
)

replace_once(
    "src/golang/middleware.go",
    '''\t\tcase panicValue = <-done:\n\t\tcase <-ctx.Done():\n\t\t\tcapture = problemResponse(504, "deadline_exceeded", "request deadline exceeded")\n\t\t}\n\t\tif panicValue != nil {\n\t\t\tslog.ErrorContext(ctx, "request handler panic", "request_id", value.RequestID, "trace_id", value.TraceID)\n\t\t\tcapture = problemResponse(500, "internal_error", "request handler failed")\n\t\t}''',
    '''\t\tcase panicValue = <-done:\n\t\tcase <-ctx.Done():\n\t\t\thandlerCapture.seal()\n\t\t\tkind := OperationFailureCancelled\n\t\t\tstatus := 499\n\t\t\tcode := "request_cancelled"\n\t\t\tdetail := "request was cancelled"\n\t\t\terrorType := "Canceled"\n\t\t\tif errors.Is(ctx.Err(), context.DeadlineExceeded) {\n\t\t\t\tkind = OperationFailureDeadlineExceeded\n\t\t\t\tstatus = http.StatusGatewayTimeout\n\t\t\t\tcode = "deadline_exceeded"\n\t\t\t\tdetail = "request deadline exceeded"\n\t\t\t\terrorType = "DeadlineExceeded"\n\t\t\t}\n\t\t\tfailure := newOperationFailure(\n\t\t\t\tkind,\n\t\t\t\tOperationDescriptor{Transport: OperationTransportHTTP, Scope: OperationScopeRequest, Name: "middleware.handler"},\n\t\t\t\tvalue,\n\t\t\t\terrorType,\n\t\t\t)\n\t\t\treportOperationFailure(ctx, s.deps.OperationFailureReporter, failure)\n\t\t\tcapture = problemResponse(status, code, detail)\n\t\t}\n\t\tif panicValue != nil {\n\t\t\tfailure := newOperationFailure(\n\t\t\t\tOperationFailurePanic,\n\t\t\t\tOperationDescriptor{Transport: OperationTransportHTTP, Scope: OperationScopeRequest, Name: "middleware.handler"},\n\t\t\t\tvalue,\n\t\t\t\tsafeErrorType(panicValue, "panic"),\n\t\t\t)\n\t\t\treportOperationFailure(ctx, s.deps.OperationFailureReporter, failure)\n\t\t\tcapture = problemResponse(500, "internal_error", "request handler failed")\n\t\t}''',
)

replace_once(
    "src/golang/middleware.go",
    '''\t\tslog.InfoContext(ctx, "request finished", "request_id", value.RequestID, "trace_id", value.TraceID, "status", status, "duration_ms", duration.Milliseconds())\n\t\tcopyResponse(writer, status, headers, body)''',
    '''\t\tslog.InfoContext(ctx, "request finished", "request_id", value.RequestID, "trace_id", value.TraceID, "status", status, "duration_ms", duration.Milliseconds())\n\t\tresponseCommitted = true\n\t\tcopyResponse(writer, status, headers, body)''',
)

replace_once(
    "src/golang/middleware.go",
    '''type bufferedResponse struct {\n\theader http.Header\n\tstatus int\n\tbody   bytes.Buffer\n}\n\nfunc newBufferedResponse() *bufferedResponse {\n\treturn &bufferedResponse{header: make(http.Header), status: http.StatusOK}\n}\nfunc (r *bufferedResponse) Header() http.Header { return r.header }\nfunc (r *bufferedResponse) WriteHeader(status int) {\n\tif r.status == http.StatusOK {\n\t\tr.status = status\n\t}\n}\nfunc (r *bufferedResponse) Write(body []byte) (int, error) { return r.body.Write(body) }\nfunc (r *bufferedResponse) snapshot() (int, http.Header, []byte) {\n\treturn r.status, r.header.Clone(), append([]byte(nil), r.body.Bytes()...)\n}''',
    '''type bufferedResponse struct {\n\tmu     sync.Mutex\n\theader http.Header\n\tstatus int\n\tbody   bytes.Buffer\n\tsealed bool\n}\n\nfunc newBufferedResponse() *bufferedResponse {\n\treturn &bufferedResponse{header: make(http.Header), status: http.StatusOK}\n}\nfunc (r *bufferedResponse) Header() http.Header { return r.header }\nfunc (r *bufferedResponse) WriteHeader(status int) {\n\tr.mu.Lock()\n\tdefer r.mu.Unlock()\n\tif r.sealed {\n\t\treturn\n\t}\n\tif r.status == http.StatusOK {\n\t\tr.status = status\n\t}\n}\nfunc (r *bufferedResponse) Write(body []byte) (int, error) {\n\tr.mu.Lock()\n\tdefer r.mu.Unlock()\n\tif r.sealed {\n\t\treturn 0, http.ErrHandlerTimeout\n\t}\n\treturn r.body.Write(body)\n}\nfunc (r *bufferedResponse) snapshot() (int, http.Header, []byte) {\n\tr.mu.Lock()\n\tdefer r.mu.Unlock()\n\treturn r.status, r.header.Clone(), append([]byte(nil), r.body.Bytes()...)\n}\nfunc (r *bufferedResponse) seal() {\n\tr.mu.Lock()\n\tr.sealed = true\n\tr.mu.Unlock()\n}''',
)

replace_once(
    "src/golang/integrations.go",
    '''\tSchemaCapture    SchemaCapture\n\tRateLimiter      RateLimiter''',
    '''\tSchemaCapture    SchemaCapture\n\tOperationFailureReporter OperationFailureReporter\n\tRateLimiter      RateLimiter''',
)

replace_once(
    "src/golang/otel.go",
    '''func cloneOresFields(source map[string]any) map[string]any {''',
    '''// WithOresLogContext installs the canonical allow-listed ores-otel frame\n// before policy hooks run and again after authentication enriches the actor.\nfunc WithOresLogContext(parent context.Context, value RequestContext) context.Context {\n\tif parent == nil {\n\t\tparent = context.Background()\n\t}\n\treturn nextloggers.WithLogContext(parent, ToOresLogContext(value))\n}\n\nfunc cloneOresFields(source map[string]any) map[string]any {''',
)

replace_once(
    "src/golang/middleware_test.go",
    '''import (\n\t"context"\n\t"net/http"\n\t"net/http/httptest"\n\t"testing"\n)''',
    '''import (\n\t"context"\n\t"errors"\n\t"net/http"\n\t"net/http/httptest"\n\t"strings"\n\t"testing"\n\t"time"\n\n\tnextloggers "github.com/ores-otel/ores.otel.log/sdk/go"\n)''',
)

with Path("src/golang/middleware_test.go").open("a", encoding="utf-8") as handle:
    handle.write(r'''

type lifecycleReport struct {
	failure      OperationFailure
	requestID    string
	userID       string
	logRequestID any
	logUserID    any
}

type panicFinishedTelemetry struct{}

func (panicFinishedTelemetry) Started(context.Context, RequestContext, *http.Request) {}
func (panicFinishedTelemetry) Finished(context.Context, RequestContext, *http.Request, int, time.Duration) {
	panic("private telemetry detail")
}

func TestAuthenticationPanicIsContainedInsideBaseRequestAndLogContext(t *testing.T) {
	reports := make(chan lifecycleReport, 1)
	stack, err := New(testConfig(), Dependencies{
		AuthVerifier: authVerifierFunc(func(ctx context.Context, _ *http.Request, value RequestContext) (AuthDecision, error) {
			current, ok := CurrentContext(ctx)
			if !ok || current.RequestID != value.RequestID {
				t.Fatalf("missing base request context: %#v", current)
			}
			logContext, ok := nextloggers.LogContextFrom(ctx)
			if !ok || logContext.Fields["request.id"] != value.RequestID {
				t.Fatalf("missing base ores-otel context: %#v", logContext)
			}
			panic("private authentication detail")
		}),
		OperationFailureReporter: func(ctx context.Context, failure OperationFailure) {
			current, _ := CurrentContext(ctx)
			logContext, _ := nextloggers.LogContextFrom(ctx)
			reports <- lifecycleReport{
				failure:      failure,
				requestID:    current.RequestID,
				userID:       current.UserID,
				logRequestID: logContext.Fields["request.id"],
				logUserID:    logContext.Fields["user.id"],
			}
		},
	})
	if err != nil {
		t.Fatal(err)
	}

	handler := stack.Wrap(http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
		t.Fatal("handler must not run")
	}))
	request := httptest.NewRequest(http.MethodGet, "http://example.test/profile", nil)
	request.Header.Set("Accept", "application/json")
	request.Header.Set("X-Request-ID", "auth-panic")
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusInternalServerError {
		t.Fatalf("status %d: %s", response.Code, response.Body.String())
	}
	if response.Header().Get("X-Request-ID") != "auth-panic" {
		t.Fatalf("missing request correlation: %#v", response.Header())
	}
	if strings.Contains(response.Body.String(), "private authentication detail") {
		t.Fatal("panic detail leaked into response")
	}
	report := <-reports
	if report.failure.Kind != OperationFailurePanic || report.failure.RequestID != "auth-panic" {
		t.Fatalf("unexpected failure: %#v", report.failure)
	}
	if report.requestID != "auth-panic" || report.logRequestID != "auth-panic" {
		t.Fatalf("reporter lost base context: %#v", report)
	}
}

func TestFinalizationPanicRetainsAuthenticatedActorContext(t *testing.T) {
	reports := make(chan lifecycleReport, 1)
	stack, err := New(testConfig(), Dependencies{
		AuthVerifier: authVerifierFunc(func(context.Context, *http.Request, RequestContext) (AuthDecision, error) {
			return AuthDecision{UserID: "user-42", TenantID: "tenant-7", Claims: map[string]string{"otel.plan": "pro", "private": "drop"}}, nil
		}),
		Telemetry: panicFinishedTelemetry{},
		OperationFailureReporter: func(ctx context.Context, failure OperationFailure) {
			current, _ := CurrentContext(ctx)
			logContext, _ := nextloggers.LogContextFrom(ctx)
			reports <- lifecycleReport{
				failure:      failure,
				requestID:    current.RequestID,
				userID:       current.UserID,
				logRequestID: logContext.Fields["request.id"],
				logUserID:    logContext.Fields["user.id"],
			}
		},
	})
	if err != nil {
		t.Fatal(err)
	}

	handler := stack.Wrap(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		current, ok := CurrentContext(request.Context())
		if !ok || current.UserID != "user-42" || current.TenantID != "tenant-7" {
			t.Fatalf("missing authenticated request context: %#v", current)
		}
		logContext, ok := nextloggers.LogContextFrom(request.Context())
		if !ok || logContext.Fields["user.id"] != "user-42" || logContext.Fields["tenant.id"] != "tenant-7" {
			t.Fatalf("missing authenticated ores-otel context: %#v", logContext)
		}
		writer.Header().Set("Content-Type", "application/json")
		_, _ = writer.Write([]byte(`{"ok":true}`))
	}))
	request := httptest.NewRequest(http.MethodGet, "http://example.test/profile", nil)
	request.Header.Set("Accept", "application/json")
	request.Header.Set("X-Request-ID", "finish-panic")
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusInternalServerError {
		t.Fatalf("status %d: %s", response.Code, response.Body.String())
	}
	if strings.Contains(response.Body.String(), "private telemetry detail") {
		t.Fatal("telemetry panic detail leaked into response")
	}
	report := <-reports
	if report.userID != "user-42" || report.logUserID != "user-42" {
		t.Fatalf("reporter lost authenticated actor context: %#v", report)
	}
}

func TestDeadlineSealsHandlerBufferAgainstLateWrites(t *testing.T) {
	config := testConfig()
	config.Settings.TimeoutMS = 5
	reports := make(chan OperationFailure, 1)
	stack, err := New(config, Dependencies{
		OperationFailureReporter: func(_ context.Context, failure OperationFailure) {
			reports <- failure
		},
	})
	if err != nil {
		t.Fatal(err)
	}

	lateWrite := make(chan error, 1)
	handler := stack.Wrap(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
		time.Sleep(25 * time.Millisecond)
		_, writeErr := writer.Write([]byte("late response must be rejected"))
		lateWrite <- writeErr
	}))
	request := httptest.NewRequest(http.MethodGet, "http://example.test/slow", nil)
	request.Header.Set("Accept", "application/json")
	request.Header.Set("X-Request-ID", "deadline-1")
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusGatewayTimeout {
		t.Fatalf("status %d: %s", response.Code, response.Body.String())
	}
	failure := <-reports
	if failure.Kind != OperationFailureDeadlineExceeded || failure.RequestID != "deadline-1" {
		t.Fatalf("unexpected timeout failure: %#v", failure)
	}
	select {
	case writeErr := <-lateWrite:
		if !errors.Is(writeErr, http.ErrHandlerTimeout) {
			t.Fatalf("late write error = %v", writeErr)
		}
	case <-time.After(250 * time.Millisecond):
		t.Fatal("late handler did not finish")
	}
	if strings.Contains(response.Body.String(), "late response") {
		t.Fatal("late handler write mutated completed response")
	}
}
''')
