# NGINX execution target

This directory documents the NGINX boundary for ORES middleware. NGINX is an
execution target for edge/transport capabilities; it does not own middleware
policy or application authorization.

Consumer `*-infra` repositories should render their NGINX configuration from
reviewed `.ores-mw.toml` plus the independently validated ORES contracts. Keep
consumer order, route placement, trusted proxy CIDRs, upstreams, and environment
values outside this repository.

`ores-middleware.conf.example` demonstrates:

- trusted forwarded-client handling;
- request and trace propagation;
- route-specific rate limits with separate named zones;
- body limits, timeouts, security headers, and structured access logging;
- explicit `429` responses for edge rate-limit rejection.

The example intentionally requires a separate trusted-proxy include instead of
shipping a permissive CIDR. Production must fail closed if the deployment has
not supplied its trusted proxy set.

Do not use the example rates as production policy. Strict principal/account,
recovery, payment, ledger, mutation, or job-admission limits remain coordinated
application decisions under the rate-limit V2 contract.
