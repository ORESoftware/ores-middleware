package oresmiddleware

import (
	"encoding/json"
	"net/http"
	"strconv"
)

const (
	UnmatchedRouteErrorCode   = "ores.route.unmatched"
	UnmatchedRouteProblemType = "urn:ores:error:route-unmatched"
	UnmatchedRouteTitle       = "No route matched"
	UnmatchedRouteDetail      = "The request target is not handled by this server."
)

// FallthroughOptions configures the outermost server/router fall-through.
// The zero value uses the standards-correct HTTP 404 when the intended origin
// was reached but no route claims the target. Set MisdirectedAuthority only
// when the request was received on an origin/connection context for which the
// server is not authoritative. A known route with an unsupported method remains
// a router-owned 405 response with Allow.
type FallthroughOptions struct {
	MisdirectedAuthority bool
}

type unmatchedRouteProblem struct {
	Type   string `json:"type"`
	Title  string `json:"title"`
	Status int    `json:"status"`
	Code   string `json:"code"`
	Detail string `json:"detail"`
}

func fallthroughStatus(options FallthroughOptions) int {
	if options.MisdirectedAuthority {
		return http.StatusMisdirectedRequest
	}
	return http.StatusNotFound
}

func encodeFallthroughProblem(options FallthroughOptions) (int, []byte) {
	status := fallthroughStatus(options)
	body, err := json.Marshal(unmatchedRouteProblem{
		Type:   UnmatchedRouteProblemType,
		Title:  UnmatchedRouteTitle,
		Status: status,
		Code:   UnmatchedRouteErrorCode,
		Detail: UnmatchedRouteDetail,
	})
	if err != nil {
		panic("oresmiddleware: static fallthrough problem must serialize")
	}
	return status, body
}

// FinalFallthroughHandler returns the canonical ORES last handler. It does not
// inspect or echo the request target, route inventory, or query string. The
// stable problem code distinguishes router fall-through from other 404s.
func FinalFallthroughHandler(options FallthroughOptions) http.Handler {
	status, body := encodeFallthroughProblem(options)
	contentLength := strconv.Itoa(len(body))

	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		w.Header().Set("Content-Length", contentLength)
		w.Header().Set("Content-Type", "application/problem+json; charset=utf-8")
		w.Header().Set("X-Content-Type-Options", "nosniff")
		w.WriteHeader(status)
		if r.Method == http.MethodHead {
			return
		}
		_, _ = w.Write(body)
	})
}

// DefaultFinalFallthroughHandler uses HTTP 404 plus ores.route.unmatched.
func DefaultFinalFallthroughHandler() http.Handler {
	return FinalFallthroughHandler(FallthroughOptions{})
}
