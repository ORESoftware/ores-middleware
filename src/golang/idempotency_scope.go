package oresmiddleware

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"strings"
	"unicode/utf8"
)

// IdempotencyScope has independent TypeSpec and JSON Schema peer authorities in
// contracts/idempotency-scope.*. Query is the raw query without the leading '?'.
type IdempotencyScope struct {
	ServiceName    string `json:"serviceName"`
	TenantID       string `json:"tenantId"`
	UserID         string `json:"userId"`
	Method         string `json:"method"`
	Path           string `json:"path"`
	Query          string `json:"query"`
	IdempotencyKey string `json:"idempotencyKey"`
}

// ScopedIdempotencyKey isolates replay by service, authenticated identity, and
// target. It does not implement distributed single-flight or body conflict
// detection. Never fall back to unscoped v1 keys on a cache miss.
func ScopedIdempotencyKey(scope IdempotencyScope) (string, error) {
	fields := []string{scope.ServiceName, scope.TenantID, scope.UserID, scope.Method, scope.Path, scope.Query, scope.IdempotencyKey}
	if scope.ServiceName == "" || scope.Method == "" || scope.IdempotencyKey == "" || !strings.HasPrefix(scope.Path, "/") {
		return "", errors.New("invalid idempotency scope")
	}
	hash := sha256.New()
	_, _ = hash.Write([]byte("ores.middleware.idempotency/v2\x00"))
	for _, value := range fields {
		if !utf8.ValidString(value) || uint64(len(value)) > uint64(^uint32(0)) {
			return "", errors.New("idempotency scope must contain bounded UTF-8")
		}
		var prefix [4]byte
		binary.BigEndian.PutUint32(prefix[:], uint32(len(value)))
		_, _ = hash.Write(prefix[:])
		_, _ = hash.Write([]byte(value))
	}
	return "ores:idempotency:v2:" + hex.EncodeToString(hash.Sum(nil)), nil
}
