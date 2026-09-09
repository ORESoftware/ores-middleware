# Runtime and contract hardening — DEN-3176 / DEN-3959

This is an implementation/evidence inventory, not a blanket compatibility or
production-readiness certification. A descriptor or adapter alias is not proof
that a framework's parsing, context, cancellation, streaming, or upgrade
lifecycle is intercepted correctly. See the exact PR-head Actions results;
passing tests from a different head are not release evidence.

## Changes and boundaries

`hapiHandler` surrounds the actual Fetch-style route handler. The legacy
`hapiLifecycle` is deprecated and admission-only; its synthetic admission
response must never be counted as actual route completion. A middleware-produced
204 now stops routing unless the continuation callback actually ran. Denials
use Hapi takeover, headers use `header()`, and separate Set-Cookie values remain
separate. Parsed JSON scalars, falsy values, text and bytes are preserved. Hapi
must enforce `payload.maxBytes` before parsing. This is buffered HTTP, not an
SSE/WebSocket upgrade adapter or a network-send completion observer.

`fetchHandler` (also exported as Node/Bun/Deno/Next route aliases) preserves extra
runtime arguments by identity, including promised Next route params. It calls
the handler with the Request supplied by middleware and rejects duplicate
dispatch and non-Response results. Next Proxy/Middleware continuation semantics
and Edge portability are separate from route-handler compatibility.

The TypeScript portable core drains a bounded body before JSON/contract readers
and dispatch. It does not trust Content-Length as a byte limit. A stalled or
cancelled body fails closed without awaiting a malicious producer's cancellation
callback. Buffering is intentional for this admission path; streaming uploads
need a separate backpressure-aware interface. Hook-wide deadlines and exporter
cancellation remain separate gaps: the body and handler have bounded phases,
not yet one enforced end-to-end wall-clock budget.

Go and TypeScript replay keys use service, authenticated tenant/user, method,
escaped path, raw query and caller key. Domain-separated SHA-256 hashes seven
big-endian uint32-length-prefixed UTF-8 fields. Shared independently computed
vectors cover every field, delimiter ambiguity, Unicode and encoded paths.
Go refreshes replay correlation and drops stale trace context. There is no
fallback to unsafe unscoped cache entries. Rollouts therefore intentionally cold
miss old replay caches. This does NOT implement single-flight, payload-conflict
rejection, distributed atomic reservations, or exactly-once effects. Rust/Gleam
must adopt and pass the same codec before sharing a v2 replay store.

## Evidence matrix

| Boundary | Implementation or test entrypoint | Not yet established by this change |
| --- | --- | --- |
| Node Fetch | Generic handler + installed-core negative tests | Full streaming response policies |
| Hapi | Real Hapi inject tests plus strict adapter regression doubles | SSE, WebSocket upgrades, transport-send completion |
| Bun / Deno | Native runtime and loopback HTTP conformance workflow | All APIs, WebSockets, edge deployments, every version |
| Express / Nest Express | Existing native response observer and request carriers retained | Unparsed request streams, abort-versus-finish classification, every response policy |
| Nest Fastify | No concrete installer certified here | Fastify hooks and response lifecycle |
| Next.js | Generic route wrapper preserves contextual arguments | Next Proxy continuation, Edge, full Next server integration |
| Hono | Existing context middleware bridge retained | Full real-framework/body-replacement and streaming conformance |
| Go | net/http stack, scoped replay integration and race-enabled vectors | Flusher/Hijacker/upgrade transparency of the buffered writer |
| Gleam / OTP | Existing gleam_http/Mist/Wisp/Cowboy/OTP functions forward the common middleware | Real installed server conversions, limits, mailbox/backpressure and cancellation |
| Rust | Concrete Axum/MASH/Leptos/Dioxus installers; generic operation guards | Universal Tower readiness, Actix/Hyper/Warp/Poem/Rocket installation |
| TCP / WebSocket | Existing operation-scoped failure/context abstractions | Wire framing, maximum message size, fragment reassembly, socket closure, message-to-message tenant isolation |

## TJSV is an executable gate, not just a dependency

`scripts/check-tjsv-contracts.mjs` verifies the immutable tool revision in
`contracts/tjsv-toolchain.json`, compiles TypeSpec with the pinned TJSV toolchain,
and compares against separately handwritten Draft 2020-12 schemas. Generated
schemas, IR, reports and temporary instance corpora go only under `target/tjsv`.
No command can overwrite either peer authority. The script requires complete
expected declaration inventory, positive and negative examples, synthesized
probes, zero unexplained findings, verified digest-bound IR, and intentional
schema-drift rejection. Compiler crashes are not accepted as negative-control
success. Failure artifacts are retained.

The first hosted run found eight structural discrepancies in the existing
docs-serving pair: `additionalProperties` versus emitted
`unevaluatedProperties`, and an emitted `RecordString` declaration/reference
versus the handwritten inline headers map. The 242 tested instance verdicts
agreed, but that does not discharge the structural gate. These findings remain
visible and release-blocking, not ignored or silently rewritten. The newly
introduced idempotency scope explicitly chooses `unevaluatedProperties: false`
in its handwritten schema; for its simple non-composed object this preserves
the intended closed-object contract. Recompilation must verify that decision.

This initial gate covers docs-serving and replay-scope data contracts, not the
entire repository's config, function bodies, language exports or transport
state machines. TJSV evidence and native conformance complement one another;
neither alone proves behavior of every runtime adapter. Existing independent
contract/persistence/runtime audit workflows remain enabled.

## Remaining high-priority acceptance work

1. Resolve the docs-serving structural peer discrepancies with an explicit
   reviewed representation decision and unchanged acceptance semantics; do not
   baseline the generated output over the handwritten source. Expand TJSV to
   all public/config/persistence contracts and consumer release evidence.
2. Correct TypeScript rate-limit identity ordering and untrusted peer-header
   use, enforce shared rate-limit core/provenance, bound in-memory stores,
   and add request-body conflict plus concurrent replay reservation tests.
3. Install and exercise real Express/Nest/Fastify/Next/Hono/Gleam servers and
   Rust framework-specific services, with native TCP/WebSocket adversarial
   framing, cancellation, backpressure and listener-survival tests. Add paired
   `*-test` consumer deployments before broad support claims.
4. Review the pinned TJSV dependency audit: its first npm install reported three
   high-severity advisories. Runtime safety and toolchain supply-chain safety
   are separate gates. No automatic audit fix or unreviewed version bump.

No credentials are included in source, workflows, test fixtures or artifacts.
No tokens were revoked, and no existing work/history was discarded.
