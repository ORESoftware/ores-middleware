package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"reflect"
	"sort"
	"strings"
	"time"

	"ores.generated.runtime.witness/persistence"
)

const witnessSchema = "ores.generated-runtime-witness/v1"

type fixture struct {
	Schema         string     `json:"schema"`
	Model          string     `json:"model"`
	WireFields     []string   `json:"wireFields"`
	RequiredFields []string   `json:"requiredFields"`
	OptionalFields []string   `json:"optionalFields"`
	Statuses       []string   `json:"statuses"`
	Cases          []testCase `json:"cases"`
}

type testCase struct {
	ID     string          `json:"id"`
	Expect string          `json:"expect"`
	Value  json.RawMessage `json:"value"`
}

type caseResult struct {
	ID         string         `json:"id"`
	Accepted   bool           `json:"accepted"`
	Normalized map[string]any `json:"normalized"`
}

type witness struct {
	Schema           string          `json:"schema"`
	Authority        string          `json:"authority"`
	Language         string          `json:"language"`
	Model            string          `json:"model"`
	WireFields       []string        `json:"wireFields"`
	RequiredFields   []string        `json:"requiredFields"`
	OptionalFields   []string        `json:"optionalFields"`
	Statuses         []string        `json:"statuses"`
	StatusAcceptance map[string]bool `json:"statusAcceptance"`
	Cases            []caseResult    `json:"cases"`
}

func nullJSON(value json.RawMessage) bool {
	return bytes.Equal(bytes.TrimSpace(value), []byte("null"))
}

func stringField(object map[string]json.RawMessage, name string) (string, bool) {
	raw, ok := object[name]
	if !ok || nullJSON(raw) {
		return "", false
	}
	var value string
	if json.Unmarshal(raw, &value) != nil {
		return "", false
	}
	return value, true
}

func strictDecode(raw json.RawMessage, contract fixture) (*persistence.IdempotencyRecord, bool) {
	var object map[string]json.RawMessage
	if err := json.Unmarshal(raw, &object); err != nil || object == nil {
		return nil, false
	}

	allowed := make(map[string]struct{}, len(contract.WireFields))
	for _, name := range contract.WireFields {
		allowed[name] = struct{}{}
	}
	for name := range object {
		if _, ok := allowed[name]; !ok {
			return nil, false
		}
	}
	for _, name := range contract.RequiredFields {
		if _, ok := stringField(object, name); !ok {
			return nil, false
		}
	}
	for _, name := range contract.OptionalFields {
		if rawValue, present := object[name]; present && nullJSON(rawValue) {
			return nil, false
		}
	}

	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var record persistence.IdempotencyRecord
	if err := decoder.Decode(&record); err != nil {
		return nil, false
	}
	if err := decoder.Decode(&struct{}{}); err != io.EOF {
		return nil, false
	}
	if !record.Status.Valid() {
		return nil, false
	}
	if _, err := time.Parse(time.RFC3339, record.CreatedAt); err != nil {
		return nil, false
	}
	if _, err := time.Parse(time.RFC3339, record.ExpiresAt); err != nil {
		return nil, false
	}
	return &record, true
}

func normalizedRecord(record *persistence.IdempotencyRecord) (map[string]any, error) {
	encoded, err := json.Marshal(record)
	if err != nil {
		return nil, err
	}
	var value map[string]any
	if err := json.Unmarshal(encoded, &value); err != nil {
		return nil, err
	}
	return value, nil
}

func reflectedFields() (wire, required, optional []string) {
	typeOf := reflect.TypeOf(persistence.IdempotencyRecord{})
	for index := 0; index < typeOf.NumField(); index++ {
		field := typeOf.Field(index)
		tag := field.Tag.Get("json")
		parts := strings.Split(tag, ",")
		name := parts[0]
		if name == "" || name == "-" {
			continue
		}
		wire = append(wire, name)
		if len(parts) > 1 && parts[1] == "omitempty" {
			optional = append(optional, name)
		} else {
			required = append(required, name)
		}
	}
	sort.Strings(wire)
	sort.Strings(required)
	sort.Strings(optional)
	return wire, required, optional
}

func main() {
	if len(os.Args) != 3 {
		fmt.Fprintln(os.Stderr, "usage: generated-go-witness <fixture.json> <authority>")
		os.Exit(64)
	}

	fixtureBytes, err := os.ReadFile(os.Args[1])
	if err != nil {
		panic(err)
	}
	var contract fixture
	if err := json.Unmarshal(fixtureBytes, &contract); err != nil {
		panic(err)
	}

	results := make([]caseResult, 0, len(contract.Cases))
	for _, item := range contract.Cases {
		record, accepted := strictDecode(item.Value, contract)
		var normalized map[string]any
		if accepted {
			normalized, err = normalizedRecord(record)
			if err != nil {
				panic(err)
			}
		}
		results = append(results, caseResult{ID: item.ID, Accepted: accepted, Normalized: normalized})
	}

	statusAcceptance := make(map[string]bool, len(contract.Statuses)+1)
	for _, status := range contract.Statuses {
		statusAcceptance[status] = persistence.IdempotencyStatus(status).Valid()
	}
	statusAcceptance["__unknown__"] = persistence.IdempotencyStatus("__unknown__").Valid()

	wire, required, optional := reflectedFields()
	output := witness{
		Schema:           witnessSchema,
		Authority:        os.Args[2],
		Language:         "golang",
		Model:            contract.Model,
		WireFields:       wire,
		RequiredFields:   required,
		OptionalFields:   optional,
		Statuses:         contract.Statuses,
		StatusAcceptance: statusAcceptance,
		Cases:            results,
	}
	encoder := json.NewEncoder(os.Stdout)
	encoder.SetEscapeHTML(false)
	if err := encoder.Encode(output); err != nil {
		panic(err)
	}
}
