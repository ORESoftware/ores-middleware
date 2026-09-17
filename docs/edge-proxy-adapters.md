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
| request ID | `$request_id` | random `%[uuid]` + `%[unique-id]` | replace public request IDs; do not encode raw client identity in the ID |
| W3C trace propagation | pass `traceparent`/`baggage` | pass `traceparent`/`baggage` | telemetry context is untrusted input; application OTel parsing validates it |
| route-specific rate limits | `limit_req_zone` + `limit_req` | ACLs + IPv4/IPv6-capable stick tables / maps | preserve named ORES policy identity, route/method scope, key and layer |
| request-size guard | `client_max_body_size` | limited proxy-side guard; prefer app/parser quota for exact body semantics | fail before expensive parsing when safely expressible |
| slow-client/timeouts | header/body/proxy/read/send timeouts | request/keepalive/connect/queue/client/server timeouts | consumer owns values and route overrides |
| coarse auth gate | `auth_request` when explicitly configured | deployment-specific external auth integration | may reject obviously unauthenticated requests; resource auth remains application-side |
| security headers | `add_header ... always` | `http-after-response set-header` | cover origin and locally generated error/rate-limit responses |
| retries | disabled by the generic example | consumer-owned retry rules | retry only operations explicitly classified safe/idempotent |
| access telemetry | bounded structured `log_format` | bounded custom `log-format` | omit query strings, credentials, cookies, bodies, and raw client identity |

## Trust boundary and forwarded headers

An application must see one reviewed transport-identity story, not whichever
header an upstream client happened to send.

At the admitted boundary:

1. establish the socket peer or trusted proxy chain;
2. discard untrusted `Forwarded`, `X-Forwarded-*`, `X-Real-IP`, and public
   request-ID claims that would compete with the admitted identity;
3. write the canonical forwarding/request-ID headers expected by the service;
4. never interpret tracing headers as authentication or authorization evidence.

If NGINX or HAProxy itself sits behind another proxy, that upstream hop must be
explicitly trusted using a reviewed transport mechanism. A public forwarded
header alone is never sufficient.

## Route-specific rate limiting

Different routes are expected to have different policies. A proxy adapter must
not collapse them into one global rate.

For NGINX, render one shared-memory zone per distinct named policy/rate class
and attach the appropriate zone to the generated `location` block. Policies
that share exactly the same key, rate, burst behavior, method scope, and layer
may share a zone; otherwise they remain distinct. When a child `location`
declares its own `limit_req`, do not rely on inherited parent limits: emit every
policy that must apply at that location explicitly.

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
2. Replace or clear public forwarding metadata at the trusted boundary; do not
   blindly append attacker-controlled chains.
3. Use named `limit_req_zone` entries for route/method policy classes, explicitly
   compose every limit that applies to a child location, and return `429` for
   edge rate-limit rejection.
4. Keep generic upstream retries off. A consumer may enable them only for an
   operation class that is explicitly safe to retry.
5. Bound slow-client/header/body occupancy as well as upstream connect/read/send
   timeouts.
6. Log `$uri` rather than `$request_uri`/`$args`; do not emit unrestricted
   headers, cookies, bodies, or raw client identity.
7. Keep auth subrequests coarse. Resource and tenant authorization stays in the
   application.
8. Do not place secrets in generated config, logs, variables, or error pages.

## HAProxy rules

1. Derive client identity from the socket peer or an explicitly trusted PROXY
   protocol hop, not an arbitrary inbound forwarded header.
2. Delete competing forwarded headers before writing the admitted forwarding
   values used by the application.
3. Generate request IDs independently of client IP/port; the example uses a
   random UUID.
4. Use a bounded custom log format. The stock HTTP format includes the full URI
   and therefore may expose query-string data.
5. Use ACLs to bind rate policies to exact method/path classes and IPv6-capable
   stick tables to count public client-IP policies.
6. Use `http-after-response` for headers that must also appear on HAProxy-local
   responses such as a generated `429`.
7. Use map files only as generated deployment data; the ORES contract/config is
   still authoritative.
8. Preserve application authorization and strict distributed admission checks.

## Admission gate

`scripts/check_edge_proxy_adapters.rs` is the durable admission check. It
verifies the security invariants above without introducing another schema
authority and invokes native `nginx -t` / `haproxy -c` parsing when those tools
are available. CI sets `ORES_EDGE_PROXY_REQUIRE_NATIVE=1`, so a missing native
parser or a syntax error fails the exact candidate revision. `just verify`
always runs the static gate and additionally runs whichever native parsers are
installed locally.

## Other targets

Envoy/Kubernetes Gateway remains a supported modern proxy path, especially when
service-mesh or xDS integration is useful. NGINX and HAProxy are included as
first-class non-Envoy targets because both are common, efficient, and can cover
most transport/edge middleware without embedding application semantics.
