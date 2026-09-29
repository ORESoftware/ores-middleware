# Default per-IP rate limiting

`ores-middleware` applies a baseline client-IP abuse guard by default.

## Baseline

The Rust middleware `default_config` enables one rate-limit bucket per **effective client IP**:

- policy ID: `ip-default`
- key: IP only
- algorithm: token bucket
- capacity: 5 requests
- refill: 5 requests/second
- window metadata: 1000 ms
- failure posture: `local-only`

The default deliberately does **not** include route, method, user, tenant, session, or API-key identity in the key. Those dimensions are useful for additional policy overlays, but including them in the baseline would let one source multiply its allowance by rotating routes or identities.

The token-bucket baseline permits an initial burst of up to five requests and then refills at five requests per second. Consumers that require a strict rolling-window ceiling should select the corresponding rate-limit algorithm/provider through the canonical `ores-rate-limit` / `.ores-rl.toml` policy rather than weakening or duplicating the IP baseline in application code.

## Effective client IP

Forwarded client identity is accepted only when the immediate peer is in the configured trusted-proxy CIDR set. For trusted peers the middleware may use `CF-Connecting-IP` or the first `X-Forwarded-For` address after validating it as an IP address. Otherwise the socket peer address is authoritative.

With strict forwarded-header handling enabled, forwarded identity from an untrusted peer is rejected instead of being allowed to spoof or fragment rate-limit buckets.

## Backpressure contract

Rate limiting is admission control, not an internal request queue. When the bucket is exhausted, middleware rejects early with HTTP `429` and returns the existing backpressure metadata:

- `Retry-After`
- `RateLimit-Limit`
- `RateLimit-Remaining`
- `RateLimit-Reset` when the limiter supplies a reset time
- `X-ORES-Rate-Limit-Policy`
- `X-ORES-Rate-Limit-Layer`
- `X-ORES-Rate-Limit-Decision`

If the authoritative limiter is unavailable and policy requires a denial rather than a normal exhaustion response, the middleware uses the existing degraded `503` path. `local-only` retains a bounded in-process fallback so a remote rate-limit backend outage does not silently remove admission control.

Clients should use `Retry-After` and the rate-limit metadata to slow producers before retrying. Servers should not sleep request tasks merely to wait for quota; rejecting early preserves memory, worker slots, queue depth, and downstream capacity.

## Layering

The per-IP guard is the safe baseline. Product/pricing policies remain additive and can impose independent limits for authenticated user, tenant, organization, API key, route, method, longer windows, concurrency, or cost-weighted operations. Those policies do not replace the baseline IP admission guard unless a consumer explicitly overrides it.
