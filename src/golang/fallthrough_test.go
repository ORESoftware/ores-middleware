package oresmiddleware

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
)

func TestFinalFallthroughDefaultsTo421(t *testing.T) {
	req := httptest.NewRequest(http.MethodGet, "https://example.test/private/secret?x=1", nil)
	rec := httptest.NewRecorder()
	DefaultFinalFallthroughHandler().ServeHTTP(rec, req)

	if rec.Code != http.StatusMisdirectedRequest {
		t.Fatalf("status = %d, want 421", rec.Code)
	}
	if got := rec.Header().Get("Cache-Control"); got != "no-store" {
		t.Fatalf("Cache-Control = %q, want no-store", got)
	}
	var problem map[string]any
	if err := json.Unmarshal(rec.Body.Bytes(), &problem); err != nil {
		t.Fatalf("decode problem: %v", err)
	}
	if problem["code"] != UnmatchedRouteErrorCode {
		t.Fatalf("code = %#v", problem["code"])
	}
	if strings.Contains(rec.Body.String(), "/private/secret") || strings.Contains(rec.Body.String(), "x=1") {
		t.Fatal("fallthrough body leaked request target")
	}
}

func TestFinalFallthroughSupports404Compatibility(t *testing.T) {
	req := httptest.NewRequest(http.MethodGet, "https://example.test/no-route", nil)
	rec := httptest.NewRecorder()
	FinalFallthroughHandler(FallthroughOptions{NotFoundCompatibility: true}).ServeHTTP(rec, req)

	if rec.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404", rec.Code)
	}
}

func TestFinalFallthroughHeadHasNoBodyAndAdvertisesLength(t *testing.T) {
	getReq := httptest.NewRequest(http.MethodGet, "https://example.test/no-route", nil)
	getRec := httptest.NewRecorder()
	DefaultFinalFallthroughHandler().ServeHTTP(getRec, getReq)

	headReq := httptest.NewRequest(http.MethodHead, "https://example.test/no-route", nil)
	headRec := httptest.NewRecorder()
	DefaultFinalFallthroughHandler().ServeHTTP(headRec, headReq)

	if headRec.Body.Len() != 0 {
		t.Fatalf("HEAD body length = %d, want 0", headRec.Body.Len())
	}
	advertised, err := strconv.Atoi(headRec.Header().Get("Content-Length"))
	if err != nil {
		t.Fatalf("invalid Content-Length: %v", err)
	}
	if advertised != getRec.Body.Len() {
		t.Fatalf("Content-Length = %d, GET body length = %d", advertised, getRec.Body.Len())
	}
}
