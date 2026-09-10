package oresmiddleware

import (
	"net/http"
	"net/http/httptest"
	"reflect"
	"testing"
)

func recordingAdapter(name string, order *[]string) Adapter {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			*order = append(*order, name+":before")
			next.ServeHTTP(w, r)
			*order = append(*order, name+":after")
		})
	}
}

func recordingFuncAdapter(name string, order *[]string) HandlerFuncAdapter {
	return func(next http.HandlerFunc) http.HandlerFunc {
		return func(w http.ResponseWriter, r *http.Request) {
			*order = append(*order, name+":before")
			next(w, r)
			*order = append(*order, name+":after")
		}
	}
}

func TestChainPreservesDeclarationOrder(t *testing.T) {
	var order []string
	handler := http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		order = append(order, "handler")
		w.WriteHeader(http.StatusNoContent)
	})

	wrapped := Chain(
		handler,
		recordingAdapter("request-id", &order),
		recordingAdapter("auth", &order),
		recordingAdapter("rate-limit", &order),
	)
	response := httptest.NewRecorder()
	wrapped.ServeHTTP(response, httptest.NewRequest(http.MethodGet, "https://example.test/", nil))

	want := []string{
		"request-id:before",
		"auth:before",
		"rate-limit:before",
		"handler",
		"rate-limit:after",
		"auth:after",
		"request-id:after",
	}
	if !reflect.DeepEqual(order, want) {
		t.Fatalf("unexpected middleware order: got %v want %v", order, want)
	}
}

func TestChainFuncsPreservesDeclarationOrder(t *testing.T) {
	var order []string
	handler := http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
		order = append(order, "handler")
	})

	wrapped := ChainFuncs(
		handler,
		recordingFuncAdapter("outer", &order),
		recordingFuncAdapter("inner", &order),
	)
	wrapped(httptest.NewRecorder(), httptest.NewRequest(http.MethodGet, "https://example.test/", nil))

	want := []string{"outer:before", "inner:before", "handler", "inner:after", "outer:after"}
	if !reflect.DeepEqual(order, want) {
		t.Fatalf("unexpected handler-func order: got %v want %v", order, want)
	}
}

func TestHandlerFuncAdapterFromHandler(t *testing.T) {
	var calls int
	adapter := HandlerFuncAdapterFromHandler(func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			calls++
			next.ServeHTTP(w, r)
		})
	})
	wrapped := adapter(func(http.ResponseWriter, *http.Request) { calls++ })
	wrapped(httptest.NewRecorder(), httptest.NewRequest(http.MethodGet, "https://example.test/", nil))
	if calls != 2 {
		t.Fatalf("expected adapter and handler to run once each, got %d calls", calls)
	}
}

func TestChainRejectsNilInputs(t *testing.T) {
	assertPanics := func(name string, fn func()) {
		t.Helper()
		defer func() {
			if recover() == nil {
				t.Fatalf("%s: expected panic", name)
			}
		}()
		fn()
	}

	assertPanics("nil handler", func() { Chain(nil) })
	assertPanics("nil adapter", func() { Chain(http.HandlerFunc(func(http.ResponseWriter, *http.Request) {}), nil) })
	assertPanics("nil handler func", func() { ChainFuncs(nil) })
	assertPanics("nil handler func adapter", func() { ChainFuncs(func(http.ResponseWriter, *http.Request) {}, nil) })
	assertPanics("nil adapter conversion", func() { HandlerFuncAdapterFromHandler(nil) })
}
