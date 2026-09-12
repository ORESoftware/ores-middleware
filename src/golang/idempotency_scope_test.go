package oresmiddleware

import (
	"encoding/json"
	"os"
	"testing"
)

func TestSharedIdempotencyScopeVectors(t *testing.T) {
	data, err := os.ReadFile("../../contracts/fixtures/idempotency-scope.json")
	if err != nil {
		t.Fatal(err)
	}
	var corpus struct {
		Cases []struct {
			Name        string           `json:"name"`
			Scope       IdempotencyScope `json:"scope"`
			ExpectedKey string           `json:"expectedKey"`
		} `json:"cases"`
	}
	if err := json.Unmarshal(data, &corpus); err != nil {
		t.Fatal(err)
	}
	if len(corpus.Cases) != 13 {
		t.Fatalf("expected 13 independent vectors, got %d", len(corpus.Cases))
	}
	seen := map[string]bool{}
	for _, item := range corpus.Cases {
		t.Run(item.Name, func(t *testing.T) {
			key, err := ScopedIdempotencyKey(item.Scope)
			if err != nil {
				t.Fatal(err)
			}
			if key != item.ExpectedKey {
				t.Fatalf("scope digest mismatch: %s", item.Name)
			}
			if seen[key] {
				t.Fatal("distinct scope collision")
			}
			seen[key] = true
		})
	}
}

func TestMalformedIdempotencyScopeRefused(t *testing.T) {
	base := IdempotencyScope{ServiceName: "service", TenantID: "tenant", UserID: "user", Method: "POST", Path: "/", IdempotencyKey: "key"}
	for _, scope := range []IdempotencyScope{
		{ServiceName: "service", Method: "POST", Path: "/", IdempotencyKey: ""},
		{ServiceName: "service", Method: "POST", Path: "relative", IdempotencyKey: "key"},
		{ServiceName: "service", Method: "POST", Path: "/", IdempotencyKey: string([]byte{0xff})},
	} {
		if _, err := ScopedIdempotencyKey(scope); err == nil {
			t.Fatal("malformed scope accepted")
		}
	}
	if _, err := ScopedIdempotencyKey(base); err != nil {
		t.Fatal(err)
	}
}
