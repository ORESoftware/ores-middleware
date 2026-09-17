# Edge proxy middleware adapters

NGINX and HAProxy are first-class **execution targets** for ORES middleware, not
new policy authorities. They realize the subset of the portable middleware
contract that belongs at an L7 proxy/load-balancer boundary and delegate the
rest to the application or an explicitly configured external service.

The authority split is deliberate:

- `ORESoftware/ores-middleware` owns portable middleware semantics, contracts,
  capability vocabulary, validation rules, and target guidance;
- each consumer `*-infra` repository owns selection, exact ordering, route
  placement, environment configuration, accepted hosts, TLS topology, and the
  rendered proxy configuration;
- NGINX, HAProxy, Envoy/Kubernetes Gateway, Cloudflare Workers, and ORES Rust
  proxies are execution targets, never policy authorities.

Do not copy business authorization, tenant/resource permission checks, or
transactional admission into a proxy merely because the proxy can execute an
ACL. Those remain application middleware.

## Target matrix

| Capability | NGINX | HAProxy | Required ORES behavior |
| --- | --- | --- | --- |
| trusted client IP / forwarded headers | `real_ip` + explicit trusted CIDRs | socket peer / explicitly trusted PROXY protocol + header rewrite | strip competing public forwarding metadata; derive one admitted client identity |
| Host admission | deployment-owned `map` include | deployment-owned ACL file | fail closed on an unrecognized Host before origin dispatch |
| request ID | `$request_id` | random `%[uuid]` + `%[unique-id]` | replace public request IDs; return the edge ID on responses |
| W3C trace propagation | preserve `traceparent`; clear public `tracestate`/`baggage` | preserve `traceparent`; delete public `tracestate`/`baggage` | arbitrary public baggage is not propagated into internal services by default |
| path safety | `merge_slashes on` for location matching | reject repeated-slash and dot-segment paths | route-specific security policy must not be bypassed by proxy/origin normalization disagreement |
| route-specific rate limits | `limit_req_zone` + `limit_req` | ACLs + IPv4/IPv6-capable stick tables / maps | preserve named ORES policy identity, route/method scope, key and layer |
| request-size guard | `client_max_body_size` | limited proxy-side guard; prefer app/parser quota for exact body semantics | fail before expensive parsing when safely expressible |
| slow-client/timeouts | header/body/proxy/read/send timeouts | request/keepalive/connect/queue/client/server timeouts | consumer owns values and route overrides |
| generic methods | reject TRACE/CONNECT | reject TRACE/CONNECT | tunneling/reflection requires a dedicated reviewed stack |
| security headers | hide origin copies, then `add_header ... always` | `http-after-response set-header` | cover origin and locally generated error/rate-limit responses with one canonical value |
| retries | disabled by the generic example | `retries 0`; no `retry-on`/redispatch | retry only operations explicitly classified safe/idempotent |
| upgrade/tunnel headers | stripped | stripped | WebSocket/h2c/tunnel routes require a dedicated reviewed transport stack |
| access telemetry | bounded structured `log_format` | bounded custom `log-format` | omit query strings, credentials, cookies, bodies, and raw client identity |

## Trust boundary and forwarded headers

An application must see one reviewed transport-identity story, not whichever
header an upstream client happened to send.

At the admitted boundary:

1. establish the socket peer or trusted proxy chain;
2. discard untrusted `Forwarded`, `X-Forwarded-*`, `X-Real-IP`, public
   request-ID claims, and the legacy `Proxy` request header that can become
   `HTTP_PROXY` in CGI-style environments;
3. require an explicit deployment-owned Host allowlist and reject unknown Host
   values before origin dispatch;
4. write the canonical forwarding/request-ID headers expected by the service;
5. never interpret tracing headers as authentication or authorization evidence.

If NGINX or HAProxy itself sits behind another proxy, that upstream hop must be
explicitly trusted using a reviewed transport mechanism. A public forwarded
header alone is never sufficient.

## Public tracing input

`traceparent` may be preserved for distributed correlation, but public
`tracestate` and `baggage` are cleared by the generic edge examples. They are
arbitrary client-controlled metadata and can otherwise become a propagation,
privacy, or cardinality channel across internal services. A consumer may
re-enable selected baggage/tracestate only at a trusted internal boundary with
an explicit size/key allowlist and the same OTel validation used by the
application runtime.

## Route/path canonicalization

Security policy and origin routing must agree on the path class being enforced.
NGINX location matching merges adjacent slashes in the configured baseline.
HAProxy deliberately does not normalize request paths by default, so the generic
HAProxy example rejects repeated slashes and RFC-style `.` / `..` path segments
instead of relying on experimental rewriting. Consumers that support unusual
encoded path semantics must prove proxy/application canonicalization parity in
tests before relaxing these guards.

## Route-specific rate limiting

Different routes are expected to have different policies. A proxy adapter must
not collapse them into one global rate.

For NGINX, render one shared-memory zone per distinct named policy/rate class
and attach the appropriate zone to the generated `location` block. Policies
that share exactly the same key, rate, burst behavior, method scope, and layer
may share a zone; otherwise they remain distinct. A more-specific/exact
`location` is selected instead of the generic `location /`, so every rate-limit
policy that must apply to that exact route is emitted explicitly there rather
than assuming the generic location also executes.

For HAProxy, use route/method ACLs plus stick tables and, where useful, map files
for threshold selection. Use IPv6-capable tables for public client-IP policies
so IPv6 clients cannot bypass an IPv4-only counter. If two policies require
different windows or key semantics, they must not share a counter whose
observable behavior changes the contract.

Examples in `adapters/nginx/` and `adapters/haproxy/` show two policies: a
coarse anonymous edge guard and a stricter `POST /v1/auth/login` guard. The
login route is still subject to the general guard. These are examples only;
consumer `*-infra` repositories render their own values from reviewed
policy/config.

Strict account, recovery, payment, ledger, mutation, and job-admission limits
remain coordinated application decisions as defined by the rate-limit V2
contract. Proxy limits at those routes are only coarse pre-origin flood guards
unless the reviewed contract explicitly says otherwise.

## Placement and de-duplication

A middleware item may intentionally exist at more than one layer (for example,
trace propagation), but consumers must distinguish **propagation** from
**enforcement**. The same named enforcement policy must not accidentally run at
Cloudflare, NGINX/HAProxy, a Rust proxy, and the application with the same
budget unless that multiplication is explicitly intended.

Recommended consumer layout:

```text
<org>-infra/
  .ores-mw.toml
  edge/
    middleware.*
    nginx/
      ores-middleware.conf
    haproxy/
      ores-middleware.cfg
    generated/
  modules/
    cloudflare_routing/
    kubernetes_ingress/
    load_balancer/
  environments/
    dev/
    stage/
    prod/
```

The handwritten/generator-owned proxy files consume ORES policy; they do not
become a third schema authority.

## NGINX rules

1. Require explicit trusted proxy CIDRs before honoring forwarded client IP;
   never generate `0.0.0.0/0` or `::/0` as a trusted proxy set.
2. Require an explicit Host allowlist and reject unknown Host values before
   origin dispatch.
3. Keep invalid-header rejection on, underscore-bearing public headers off, and
   slash merging on unless a consumer proves different parser semantics safe.
4. Replace or clear public forwarding metadata, public baggage/tracestate,
   legacy `Proxy`, and generic upgrade/hop-by-hop headers at the trusted edge.
5. Use named `limit_req_zone` entries for route/method policy classes, explicitly
   compose every limit that applies to an exact/specific location, and return
   `429` for edge rate-limit rejection.
6. Keep generic upstream retries off. A consumer may enable them only for an
   operation class that is explicitly safe to retry.
7. Hide origin copies of edge-owned response policy headers before emitting one
   canonical edge value, including the edge request ID.
8. Bound slow-client/header/body occupancy as well as upstream connect/read/send
   timeouts and keep logs free of query strings/credentials/raw identity.

## HAProxy rules

1. Derive client identity from the socket peer or an explicitly trusted PROXY
   protocol hop, not an arbitrary inbound forwarded header.
2. Require an explicit Host allowlist and reject unknown Host values before
   origin dispatch.
3. Reject repeated-slash and dot-segment request paths in the generic adapter so
   route policy cannot be bypassed by downstream path normalization.
4. Delete competing forwarded headers, public baggage/tracestate, legacy
   `Proxy`, and generic upgrade/hop-by-hop headers before origin dispatch.
5. Generate request IDs independently of client IP/port; return the same ID on
   the response.
6. Keep `retries 0`, forbid generic `retry-on`/redispatch, and use explicit
   `http-reuse safe` unless a reviewed operation-specific policy says otherwise.
7. Use a bounded custom log format, IPv6-capable rate tables, and
   `http-after-response` for canonical edge response headers.
8. Preserve application authorization and strict distributed admission checks.

## Admission gate

`scripts/check_edge_proxy_adapters.rs` is the durable admission check. Static
checks operate on active directives only, so comments cannot satisfy a required
security invariant. Native validation uses temporary trusted-proxy/Host fixtures
for syntax tests while the checked-in examples remain fail-closed without real
deployment allowlists. CI sets `ORES_EDGE_PROXY_REQUIRE_NATIVE=1`, so a missing
native parser or a syntax error fails the exact candidate revision. `just
verify` always runs the static gate and additionally runs whichever native
parsers are installed locally.

## Other targets

Envoy/Kubernetes Gateway remains a supported modern proxy path, especially when
service-mesh or xDS integration is useful. NGINX and HAProxy are included as
first-class non-Envoy targets because both are common, efficient, and can cover
most transport/edge middleware without embedding application semantics.
