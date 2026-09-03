package oresmiddleware

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

const (
	zeroTraceID       = "00000000000000000000000000000000"
	validTraceID      = "0123456789abcdef0123456789abcdef"
	validParentSpanID = "0123456789abcdef"
	validServerSpanID = "fedcba9876543210"
)

func traceparentRequest(traceID string) *http.Request {
	request := httptest.NewRequest(http.MethodGet, "http://example.test/trace", nil)
	request.Header.Set("Accept", "application/json")
	request.Header.Set("Traceparent", "00-"+traceID+"-"+validParentSpanID+"-01")
	return request
}

func TestInboundParentIsNotRelabelledAsResponseSpan(t *testing.T) {
	stack, err := New(testConfig(), Dependencies{})
	if err != nil {
		t.Fatal(err)
	}
	handler := stack.Wrap(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
		writer.WriteHeader(http.StatusNoContent)
	}))
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, traceparentRequest(validTraceID))

	if got := response.Header().Get("Traceparent"); got != "" {
		t.Fatalf("unexpected synthesized response traceparent %q", got)
	}
}

func TestOnlyValidTracerOwnedResponseTraceparentIsPreserved(t *testing.T) {
	valid := "00-" + validTraceID + "-" + validServerSpanID + "-01"
	tests := []struct {
		name      string
		candidate string
		expected  string
	}{
		{name: "valid", candidate: strings.ToUpper(valid), expected: valid},
		{name: "zero span", candidate: "00-" + validTraceID + "-0000000000000000-01"},
		{name: "zero trace", candidate: "00-" + zeroTraceID + "-" + validServerSpanID + "-01"},
		{name: "malformed", candidate: "00-not-hex-not-a-span-01"},
	}

	for _, item := range tests {
		t.Run(item.name, func(t *testing.T) {
			stack, err := New(testConfig(), Dependencies{})
			if err != nil {
				t.Fatal(err)
			}
			handler := stack.Wrap(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
				writer.Header().Set("Traceparent", item.candidate)
				writer.WriteHeader(http.StatusNoContent)
			}))
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, traceparentRequest(validTraceID))
			if got := response.Header().Get("Traceparent"); got != item.expected {
				t.Fatalf("traceparent=%q expected=%q", got, item.expected)
			}
		})
	}
}

func TestAllZeroInboundTraceIDIsReplaced(t *testing.T) {
	stack, err := New(testConfig(), Dependencies{})
	if err != nil {
		t.Fatal(err)
	}
	observed := make(chan string, 1)
	handler := stack.Wrap(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		context, ok := CurrentContext(request.Context())
		if !ok {
			t.Fatal("request context missing")
		}
		observed <- context.TraceID
		writer.WriteHeader(http.StatusNoContent)
	}))
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, traceparentRequest(zeroTraceID))

	traceID := <-observed
	if traceID == zeroTraceID || len(traceID) != 32 {
		t.Fatalf("invalid replacement trace ID %q", traceID)
	}
	if got := response.Header().Get("Traceparent"); got != "" {
		t.Fatalf("unexpected response traceparent %q", got)
	}
}
