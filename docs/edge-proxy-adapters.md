# Edge proxy middleware adapters

NGINX and HAProxy are first-class **execution targets** for ORES middleware, not
new policy authorities. They realize the subset of the portable middleware
contract that belongs at an L7 proxy/load-balancer boundary and delegate the
rest to the application or an explicitly configured external service.

The authority split is deliberate:

- `ORESoftware/ores-middleware` owns portable middleware semantics, contracts,
  capability vocabulary, validation rules, and target guidance;
- each consumer `*-infra` repository owns selection, exact ordering, route
  placement, environment configuration, and the rendered proxy configuration;
- NGINX, HAProxy, Envoy/Kubernetes Gateway, Cloudflare Workers, and ORES Rust
  proxies are execution targets, never policy authorities.

Do not copy business authorization, tenant/resource permission checks, or
transactional admission into a proxy merely because the proxy can execute an
ACL. Those remain application middleware.

## Target matrix

| Capability | NGINX | HAProxy | Required ORES behavior |
| --- | --- | --- | --- |
| trusted client IP / forwarded headers | `real_ip` + explicit trusted CIDRs | socket peer / PROXY protocol + header rewrite | ignore or replace untrusted forwarded identity |
| request ID | `$request_id` | `unique-id-format` / `%[unique-id]` | generate or replace at the trusted boundary |
| W3C trace propagation | pass `traceparent`/`baggage` | pass `traceparent`/`baggage` | never synthesize malformed tracing headers |
| route-specific rate limits | `limit_req_zone` + `limit_req` | ACLs + stick tables / maps | preserve named ORES policy identity and layer |
| request-size guard | `client_max_body_size` | limited proxy-side guard; prefer app/parser quota for exact body semantics | fail before expensive parsing when safely expressible |
| timeouts | proxy/read/send/connect timeouts | connect/client/server/http timeouts | consumer owns values and route overrides |
| coarse auth gate | `auth_request` when explicitly configured | deployment-specific external auth integration | may reject obviously unauthenticated requests; resource auth remains application-side |
| security headers | `add_header ... always` | `http-response set-header` | do not weaken application-required headers |
| retries | `proxy_next_upstream` | retry rules | retry only operations classified safe/idempotent by consumer policy |
| access telemetry | structured `log_format` | structured log format | redact credentials, cookies, raw identities, and bodies |

## Route-specific rate limiting

Different routes are expected to have different policies. A proxy adapter must
not collapse them into one global rate.

For NGINX, render one shared-memory zone per distinct named policy/rate class
and attach the appropriate zone to the generated `location` block. Policies
that share exactly the same key, rate, burst behavior, and layer may share a
zone; otherwise they must remain distinct.

For HAProxy, use route/method ACLs plus stick tables and, where useful, map files
for threshold selection. If two policies require different windows or key
semantics, they must not share a counter whose observable behavior changes the
contract.

Examples in `adapters/nginx/` and `adapters/haproxy/` show two policies: a
coarse anonymous edge guard and a stricter `POST /v1/auth/login` guard. These
are examples only; consumer `*-infra` repositories render their own values from
reviewed policy/config.

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

1. Require explicit trusted proxy CIDRs before honoring forwarded client IP.
2. Replace public `X-Forwarded-For`/`X-Forwarded-Proto` at the trusted boundary;
   do not blindly append attacker-controlled chains.
3. Use named `limit_req_zone` entries for route policy classes and return `429`
   for edge rate-limit rejection.
4. Keep auth subrequests coarse. Resource and tenant authorization stays in the
   application.
5. Do not place secrets in generated config, logs, variables, or error pages.

## HAProxy rules

1. Derive client identity from the socket peer or an explicitly trusted PROXY
   protocol hop, not an arbitrary inbound forwarded header.
2. Rewrite forwarded headers before dispatch to the application.
3. Use ACLs to bind rate policies to method/path classes and stick tables to
   count the intended key/window.
4. Use map files only as generated deployment data; the ORES contract/config is
   still authoritative.
5. Preserve application authorization and strict distributed admission checks.

## Other targets

Envoy/Kubernetes Gateway remains a supported modern proxy path, especially when
service-mesh or xDS integration is useful. NGINX and HAProxy are included as
first-class non-Envoy targets because both are common, efficient, and can cover
most transport/edge middleware without embedding application semantics.
