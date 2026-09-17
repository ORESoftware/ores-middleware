# HAProxy execution target

HAProxy is the second first-class non-Envoy proxy/load-balancer target for ORES
middleware. It is well suited to high-throughput L4/L7 routing, connection
limits, timeouts, request metadata normalization, and coarse route-aware rate
limiting.

Consumer `*-infra` repositories own exact middleware order, route matching,
policy values, peers/cluster topology, TLS, upstreams, and environment-specific
rendering. The shared ORES contracts remain authoritative.

`ores-middleware.cfg.example` demonstrates:

- request ID creation and forwarded-header replacement;
- W3C trace propagation;
- a general IP rate guard plus a stricter login route guard using ACLs and
  distinct stick tables;
- `429` rejection, transport timeouts, security headers, and origin routing.

For many route policies, a consumer renderer may use HAProxy map files for route
classification/threshold lookup. Map files are generated deployment data, not a
third contract authority. Policies with different windows or key semantics must
not be collapsed into one counter if doing so changes observable behavior.

Do not move resource authorization or strict distributed admission into HAProxy.
Those remain application/coordinator middleware.
