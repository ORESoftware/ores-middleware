package docsserving

import (
	"bufio"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
)

func optional(value string) string {
	if value == "-" {
		return ""
	}
	return value
}

func TestConformanceFixture(t *testing.T) {
	path := filepath.Join("..", "..", "..", "fixtures", "docs-serving-conformance.tsv")
	file, err := os.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()

	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		line := scanner.Text()
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		fields := strings.Split(line, "\t")
		if len(fields) != 11 {
			t.Fatalf("invalid fixture row (%d fields): %s", len(fields), line)
		}
		name := fields[0]
		t.Run(name, func(t *testing.T) {
			request := Request{
				Method:                fields[1],
				Path:                  fields[2],
				Accept:                optional(fields[3]),
				Format:                optional(fields[4]),
				RuntimeContractDigest: optional(fields[5]),
				DocsContractDigest:    optional(fields[6]),
			}
			decision := Decide(request)
			if string(decision.Action) != fields[7] {
				t.Fatalf("action: got %s want %s", decision.Action, fields[7])
			}
			if fields[8] == "-" {
				if decision.Status != nil {
					t.Fatalf("status: got %d want nil", *decision.Status)
				}
			} else {
				want, err := strconv.Atoi(fields[8])
				if err != nil {
					t.Fatal(err)
				}
				if decision.Status == nil || *decision.Status != want {
					t.Fatalf("status: got %v want %d", decision.Status, want)
				}
			}
			if fields[9] == "-" {
				if decision.Representation != nil {
					t.Fatalf("representation: got %s want nil", *decision.Representation)
				}
			} else if decision.Representation == nil || string(*decision.Representation) != fields[9] {
				t.Fatalf("representation: got %v want %s", decision.Representation, fields[9])
			}
			wantHead := fields[10] == "true"
			if decision.HeadOnly != wantHead {
				t.Fatalf("headOnly: got %t want %t", decision.HeadOnly, wantHead)
			}

			if decision.Action == Pass {
				if len(decision.Headers) != 0 {
					t.Fatalf("pass emitted headers: %#v", decision.Headers)
				}
			} else {
				if decision.Headers["Cache-Control"] != "no-store" {
					t.Fatal("handled response must be no-store")
				}
				if !strings.Contains(decision.Headers["Vary"], DocsFormatHeader) {
					t.Fatal("handled response must vary on docs format")
				}
			}
			if decision.Action == MethodNotAllowed && decision.Headers["Allow"] != "GET, HEAD" {
				t.Fatal("405 must expose Allow")
			}
			if decision.Representation != nil && *decision.Representation == HTML {
				if decision.Headers["X-Frame-Options"] != "DENY" {
					t.Fatal("HTML must deny framing")
				}
				if !strings.Contains(decision.Headers["Content-Security-Policy"], "frame-ancestors 'none'") {
					t.Fatal("HTML CSP must deny framing")
				}
			}
			if len(request.DocsContractDigest) == 64 && decision.Action == Serve {
				if decision.Headers[ContractDigestHeader] != request.DocsContractDigest {
					t.Fatal("verified digest header missing")
				}
			}
		})
	}
	if err := scanner.Err(); err != nil {
		t.Fatal(err)
	}
}
