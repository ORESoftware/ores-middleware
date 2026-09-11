package oresmiddleware

import "net/http"

// Adapter is the framework-neutral net/http middleware shape used by Go
// servers. The first adapter passed to Chain is the outermost middleware and
// therefore runs first on the request path and last on the response path.
type Adapter func(http.Handler) http.Handler

// HandlerFuncAdapter is the equivalent shape for codebases that still compose
// http.HandlerFunc values directly.
type HandlerFuncAdapter func(http.HandlerFunc) http.HandlerFunc

// Chain composes adapters around handler while preserving declaration order.
//
//	Chain(handler, requestID, auth, rateLimit)
//
// executes requestID -> auth -> rateLimit -> handler. This intentionally
// mirrors the useful adapter-chaining pattern used by ORESoftware/cp-go-api,
// while keeping the shared core independent of Gorilla Mux, Gin, Echo, Fiber,
// or any other router package.
func Chain(handler http.Handler, adapters ...Adapter) http.Handler {
	if handler == nil {
		panic("oresmiddleware.Chain: handler must not be nil")
	}
	for i := len(adapters) - 1; i >= 0; i-- {
		if adapters[i] == nil {
			panic("oresmiddleware.Chain: adapter must not be nil")
		}
		handler = adapters[i](handler)
	}
	return handler
}

// ChainFuncs is Chain for http.HandlerFunc middleware stacks.
func ChainFuncs(handler http.HandlerFunc, adapters ...HandlerFuncAdapter) http.HandlerFunc {
	if handler == nil {
		panic("oresmiddleware.ChainFuncs: handler must not be nil")
	}
	for i := len(adapters) - 1; i >= 0; i-- {
		if adapters[i] == nil {
			panic("oresmiddleware.ChainFuncs: adapter must not be nil")
		}
		handler = adapters[i](handler)
	}
	return handler
}

// AsAdapter exposes a Stack as ordinary net/http middleware so it can be
// inserted into Chain alongside application-owned middleware.
func AsAdapter(stack *Stack) Adapter {
	if stack == nil {
		panic("oresmiddleware.AsAdapter: stack must not be nil")
	}
	return stack.Wrap
}

// HandlerFuncAdapterFromHandler adapts the standard net/http middleware shape
// into HandlerFuncAdapter without introducing a router dependency.
func HandlerFuncAdapterFromHandler(adapter Adapter) HandlerFuncAdapter {
	if adapter == nil {
		panic("oresmiddleware.HandlerFuncAdapterFromHandler: adapter must not be nil")
	}
	return func(next http.HandlerFunc) http.HandlerFunc {
		return adapter(next).ServeHTTP
	}
}
