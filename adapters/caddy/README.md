# Caddy edge adapter

Caddy is a first-class **thin edge proxy** for ORES deployments. It is not the
application routing or middleware authority.

The deployment-owned Caddy configuration may terminate TLS, admit hosts,
normalize trusted transport identity, apply coarse flood guards, set bounded
transport timeouts, and forward traffic to the ORES Rust/Erlang router. Route
semantics, middleware ordering, Lambda revisions, contract metadata, deployment
generation identity, and provider-neutral middleware execution stay behind the
proxy.

`Caddyfile.example` is intentionally a closed local baseline. Consumer infra
must replace the example host allowlist and trusted proxy CIDRs with reviewed
deployment values. Public deployments may enable Caddy Automatic HTTPS rather
than copying certificate mechanics into the middleware layer.

## Swappability contract

Caddy, NGINX and HAProxy must all realize the same outer lifecycle:

1. validate candidate proxy configuration;
2. stage configuration for generation `N+1`;
3. activate only after the ORES router generation is healthy;
4. preserve existing connections while the old generation drains where the
   proxy supports that mechanism;
5. rollback to the previous admitted proxy configuration on activation failure.

The proxy lifecycle is subordinate to the ORES immutable deployment generation.
Changing from Caddy to NGINX or HAProxy must not change middleware order or
application route semantics.

## Provider-specific middleware

Do not implement application middleware as a Caddy module merely for
portability. A Caddy-native module is an optional optimization and must be
backed by the same semantic contract digest and conformance fixtures as the
generic JS/Wasm/native/process artifact it replaces.
