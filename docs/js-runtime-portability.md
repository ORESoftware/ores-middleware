# JavaScript runtime portability

The TypeScript package is expected to run on Node.js, Bun, and Deno through the same Fetch-oriented core. Runtime support is therefore established by executed conformance, not by the presence of aliases such as `bunHandler` or `denoHandler` alone.

## Supported runtime floor

The package metadata records the minimum versions exercised by CI:

- Node.js 22.23.1
- Bun 1.4.2
- Deno 2.9.6

Newer versions are expected to remain compatible, but the exact pinned versions above are the reproducible conformance floor.

## Required evidence

The `js-runtime-portability` workflow checks three independent surfaces on the exact pull-request head:

1. generated TypeScript artifacts produced independently from the TypeSpec and JSON Schema/OpenAPI authorities;
2. source-tree middleware behavior, including docs-serving, generic provider composition, request-context propagation, and adversarial failure/cleanup cases;
3. the self-contained `target/ts` package closure, imported and exercised under Node, Bun, and Deno rather than only under Node.

The generic/context suite requires the same normalized semantic witness from all three runtimes. It covers provider success/failure identity, consumer-selected middleware order, duplicate named stages, empty-chain identity, nested request-context restoration, captured context, callback binding, and concurrent context isolation.

The adversarial suite additionally requires thrown provider and handler identities to survive unchanged, verifies `finally` unwind order, checks cleanup after exceptions, verifies that an explicitly absent captured context clears unrelated ambient state, prevents a callback bound outside a request from inheriting a later request, and checks that caller mutation cannot modify the stored context snapshot.

## Package-boundary rule

`package.json` at the repository root and `src/ts/package.json` must expose the same subpaths and runtime-support metadata. The root package is an orchestration/development entry point while `src/ts` is the publishable package, but neither may advertise a subpath that the other silently omits.

The runtime portability workflow builds `target/ts` through the same build-target path used for packaging and then runs the packaged generic/context suite against `target/ts/dist`. This catches checkout-only module resolution and release-closure drift.

## What this does not claim

Passing these gates does not imply every Node-specific framework adapter is meaningful in Bun or Deno. Express, Koa, Fastify, Hapi, Next.js, and similar integrations retain their own framework/runtime constraints. Bun and Deno support applies to the Fetch-oriented portable core and adapters explicitly backed by that core; framework-specific adapters need their own lifecycle tests.
