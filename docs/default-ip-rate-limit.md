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

In-process fallback bucket stores are capped at **10,000 client-IP entries** across the Rust, TypeScript, Go, Elixir, Erlang, and Gleam implementations. Admission of a new IP evicts an older local bucket rather than allowing attacker-controlled source cardinality to grow process memory without bound. Authoritative distributed quota providers may use their own bounded retention policy.

The token-bucket baseline permits an initial burst of up to five requests and then refills at five requests per second. Consumers that require a strict rolling-window ceiling should select the corresponding rate-limit algorithm/provider through the canonical `ores-rate-limit` / `.ores-rl.toml` policy rather than weakening or duplicating the IP baseline in application code.

## Effective client IP

Forwarded client identity is accepted only when the immediate peer is in the configured trusted-proxy CIDR set. When `X-Forwarded-For` is present, middleware parses the entire chain, appends the transport-authenticated socket peer, and walks **right-to-left**. Known trusted proxy hops are skipped and the nearest untrusted hop becomes the effective client identity. Attacker-prepended values on the left therefore cannot become authoritative merely because the immediate proxy is trusted.

If any `X-Forwarded-For` element is malformed, resolution fails safely to the socket peer rather than partially trusting the chain. `CF-Connecting-IP` (and the compatibility `X-Real-IP` path where supported) is considered only when `X-Forwarded-For` is absent. Otherwise the socket peer address is authoritative.

Trusted-proxy membership remains necessary: an intermediary should still sanitize/overwrite forwarding metadata according to its deployment contract. The middleware chain walk is defense in depth, not permission to trust arbitrary proxies.

With strict forwarded-header handling enabled, forwarded identity from an untrusted peer is rejected instead of being allowed to spoof or fragment rate-limit buckets.

## Backpressure contract

Rate limiting is admission control, not an internal request queue. Admitted
responses expose the current budget so clients can queue and pace work before
they reach a rejection:

- `RateLimit-Policy: "ip-default";q=5;w=1`
- `RateLimit: "ip-default";r=<remaining>;t=<effective-window-seconds>`
- compatibility `RateLimit-Limit`, `RateLimit-Remaining`, and
  `RateLimit-Reset` fields

When the bucket is exhausted, middleware rejects early with HTTP `429` and
adds `Retry-After`. If both `Retry-After` and `RateLimit` are present,
`Retry-After` is the hard stop signal. ORES policy/layer/decision headers stay
available for diagnostics.

If the authoritative limiter is unavailable and policy requires a denial rather
than normal exhaustion, middleware uses the existing degraded `503` path.
`local-only` retains a bounded in-process fallback so a remote rate-limit
backend outage does not silently remove admission control.

Clients should consume the quota hints pessimistically: stop dispatch when
remaining quota reaches zero, begin pacing when it becomes low, cap unreasonable
server-provided delays, and add small resume jitter to avoid a thundering herd.
Servers should not sleep request tasks merely to wait for quota; rejecting early
preserves memory, worker slots, queue depth, and downstream capacity.

For cross-origin browser clients, the application's CORS layer must expose the
RateLimit and Retry-After response headers.

## Layering

The per-IP guard is the safe baseline. Product/pricing policies remain additive and can impose independent limits for authenticated user, tenant, organization, API key, route, method, longer windows, concurrency, or cost-weighted operations. Those policies do not replace the baseline IP admission guard unless a consumer explicitly overrides it.
