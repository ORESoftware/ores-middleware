// Package docsserving implements the routing-neutral ores.docs-serving/v1 policy.
package docsserving

import (
	"regexp"
	"sort"
	"strconv"
	"strings"
)

const (
	DocsFormatHeader     = "X-Ores-Docs-Format"
	ContractDigestHeader = "X-Ores-Contract-SHA256"
)

type Representation string

const (
	HTML        Representation = "html"
	Catalog     Representation = "catalog"
	OpenAPI     Representation = "openapi"
	OpenRPC     Representation = "openrpc"
	Connect     Representation = "connect"
	HyperSchema Representation = "hyper-schema"
)

type Action string

const (
	Pass                 Action = "pass"
	Serve                Action = "serve"
	MethodNotAllowed     Action = "method-not-allowed"
	NotAcceptable        Action = "not-acceptable"
	StoppedForEvaluation Action = "stopped-for-evaluation"
)

type Request struct {
	Method                string
	Path                  string
	Accept                string
	Format                string
	RuntimeContractDigest string
	DocsContractDigest    string
}

type Decision struct {
	Action         Action
	Status         *int
	Representation *Representation
	HeadOnly       bool
	Headers        map[string]string
}

var (
	htmlPaths = map[string]struct{}{
		"/docs/api": {},
		"/api/docs": {},
		"/api-docs": {},
	}
	pathRepresentations = map[string]Representation{
		"/api/docs.json":     Catalog,
		"/api-docs.json":     Catalog,
		"/openapi.json":      OpenAPI,
		"/openrpc.json":      OpenRPC,
		"/connect.json":      Connect,
		"/hyper-schema.json": HyperSchema,
	}
	representations = map[string]Representation{
		string(HTML):        HTML,
		string(Catalog):     Catalog,
		string(OpenAPI):     OpenAPI,
		string(OpenRPC):     OpenRPC,
		string(Connect):     Connect,
		string(HyperSchema): HyperSchema,
	}
	mediaTypes = map[string]Representation{
		"text/html":                          HTML,
		"application/vnd.ores.api-docs+json": Catalog,
		"application/json":                   Catalog,
		"application/vnd.oai.openapi+json":   OpenAPI,
		"application/openapi+json":           OpenAPI,
		"application/openrpc+json":           OpenRPC,
		"application/vnd.ores.connect+json":  Connect,
		"application/schema+json":            HyperSchema,
	}
	contentTypes = map[Representation]string{
		HTML:        "text/html; charset=utf-8",
		Catalog:     "application/vnd.ores.api-docs+json; charset=utf-8",
		OpenAPI:     "application/vnd.oai.openapi+json; charset=utf-8",
		OpenRPC:     "application/openrpc+json; charset=utf-8",
		Connect:     "application/vnd.ores.connect+json; charset=utf-8",
		HyperSchema: "application/schema+json; charset=utf-8",
	}
	sha256Hex = regexp.MustCompile(`^[0-9a-f]{64}$`)
)

func intPointer(value int) *int { return &value }

func representationPointer(value Representation) *Representation { return &value }

func baseHeaders(contentType string) map[string]string {
	return map[string]string{
		"Cache-Control":          "no-store",
		"Pragma":                 "no-cache",
		"X-Content-Type-Options": "nosniff",
		"Referrer-Policy":        "no-referrer",
		"Permissions-Policy":     "camera=(), microphone=(), geolocation=()",
		"Vary":                   "Accept, " + DocsFormatHeader,
		"Content-Type":           contentType,
	}
}

func headersForRepresentation(representation Representation, digest string) map[string]string {
	headers := baseHeaders(contentTypes[representation])
	if representation == HTML {
		headers["X-Frame-Options"] = "DENY"
		headers["Content-Security-Policy"] = "default-src 'none'; style-src 'unsafe-inline'; img-src 'none'; frame-ancestors 'none'; base-uri 'none'; object-src 'none'; form-action 'none'; connect-src 'none'; script-src 'none'"
	}
	if digest != "" {
		headers[ContractDigestHeader] = digest
	}
	return headers
}

func reject(action Action, status int, extra map[string]string) Decision {
	headers := baseHeaders("application/json; charset=utf-8")
	for key, value := range extra {
		headers[key] = value
	}
	return Decision{
		Action:   action,
		Status:   intPointer(status),
		HeadOnly: false,
		Headers:  headers,
	}
}

type mediaRange struct {
	media   string
	quality float64
	index   int
}

func parseAccept(value string) []mediaRange {
	if strings.TrimSpace(value) == "" {
		return nil
	}
	ranges := make([]mediaRange, 0)
	for index, rawPart := range strings.Split(value, ",") {
		parts := strings.Split(rawPart, ";")
		media := strings.ToLower(strings.TrimSpace(parts[0]))
		if media == "" {
			continue
		}
		quality := 1.0
		valid := true
		for _, rawParameter := range parts[1:] {
			pair := strings.SplitN(rawParameter, "=", 2)
			if strings.ToLower(strings.TrimSpace(pair[0])) != "q" {
				continue
			}
			if len(pair) != 2 {
				valid = false
				break
			}
			parsed, err := strconv.ParseFloat(strings.TrimSpace(pair[1]), 64)
			if err != nil || parsed < 0 || parsed > 1 {
				valid = false
				break
			}
			quality = parsed
		}
		if !valid || quality <= 0 {
			continue
		}
		ranges = append(ranges, mediaRange{media: media, quality: quality, index: index})
	}
	sort.SliceStable(ranges, func(i, j int) bool {
		if ranges[i].quality == ranges[j].quality {
			return ranges[i].index < ranges[j].index
		}
		return ranges[i].quality > ranges[j].quality
	})
	return ranges
}

func mediaRepresentation(media string) (Representation, bool) {
	if media == "*/*" {
		return HTML, true
	}
	if media == "application/*" {
		return Catalog, true
	}
	representation, ok := mediaTypes[media]
	return representation, ok
}

func negotiateGeneric(accept string) (Representation, bool) {
	ranges := parseAccept(accept)
	if len(ranges) == 0 {
		if strings.TrimSpace(accept) == "" {
			return HTML, true
		}
		return "", false
	}
	for _, item := range ranges {
		if representation, ok := mediaRepresentation(item.media); ok {
			return representation, true
		}
	}
	return "", false
}

func acceptsRepresentation(accept string, representation Representation) bool {
	if strings.TrimSpace(accept) == "" {
		return true
	}
	ranges := parseAccept(accept)
	if len(ranges) == 0 {
		return false
	}
	for _, item := range ranges {
		if item.media == "*/*" {
			return true
		}
		if representation != HTML && (item.media == "application/*" || item.media == "application/json") {
			return true
		}
		if candidate, ok := mediaRepresentation(item.media); ok && candidate == representation {
			return true
		}
	}
	return false
}

func normalizedFormat(value string) (Representation, bool, bool) {
	trimmed := strings.ToLower(strings.TrimSpace(value))
	if trimmed == "" {
		return "", false, true
	}
	representation, ok := representations[trimmed]
	return representation, true, ok
}

func digestFailure(runtimeDigest string, docsDigest string) bool {
	runtimeDigest = strings.TrimSpace(runtimeDigest)
	docsDigest = strings.TrimSpace(docsDigest)
	runtimePresent := runtimeDigest != ""
	docsPresent := docsDigest != ""
	if runtimePresent && !sha256Hex.MatchString(runtimeDigest) {
		return true
	}
	if docsPresent && !sha256Hex.MatchString(docsDigest) {
		return true
	}
	return runtimePresent && (!docsPresent || runtimeDigest != docsDigest)
}

// Decide evaluates a normalized host request without registering a route or loading a body.
func Decide(request Request) Decision {
	path := strings.SplitN(request.Path, "?", 2)[0]
	_, generic := htmlPaths[path]
	fixedRepresentation, fixed := pathRepresentations[path]
	if !generic && !fixed {
		return Decision{Action: Pass, HeadOnly: false, Headers: map[string]string{}}
	}

	method := strings.ToUpper(request.Method)
	if method != "GET" && method != "HEAD" {
		return reject(MethodNotAllowed, 405, map[string]string{"Allow": "GET, HEAD"})
	}

	runtimeDigest := strings.TrimSpace(request.RuntimeContractDigest)
	docsDigest := strings.TrimSpace(request.DocsContractDigest)
	if digestFailure(runtimeDigest, docsDigest) {
		return reject(StoppedForEvaluation, 503, nil)
	}

	format, formatPresent, formatValid := normalizedFormat(request.Format)
	if !formatValid {
		return reject(NotAcceptable, 406, nil)
	}

	var representation Representation
	if generic {
		if formatPresent {
			representation = format
		} else {
			var ok bool
			representation, ok = negotiateGeneric(request.Accept)
			if !ok {
				return reject(NotAcceptable, 406, nil)
			}
		}
	} else {
		representation = fixedRepresentation
		if formatPresent && format != representation {
			return reject(NotAcceptable, 406, nil)
		}
	}

	if !acceptsRepresentation(request.Accept, representation) {
		return reject(NotAcceptable, 406, nil)
	}

	return Decision{
		Action:         Serve,
		Status:         intPointer(200),
		Representation: representationPointer(representation),
		HeadOnly:       method == "HEAD",
		Headers:        headersForRepresentation(representation, docsDigest),
	}
}
