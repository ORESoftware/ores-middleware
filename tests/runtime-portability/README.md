# Runtime portability probes

These probes are executable semantic witnesses, not unit-test substitutes.

- `docs-serving-smoke.mjs` checks portable docs-serving behavior.
- `generic-context-smoke.mjs` checks the source-tree generic provider/composition and request-context surfaces.
- `adversarial-boundary-smoke.mjs` checks source-tree failure identity and context cleanup.
- `packaged-generic-context-smoke.mjs` runs the same class of generic/context checks against the self-contained `target/ts` build artifact.
- `packaged-adversarial-boundary-smoke.mjs` checks the packaged artifact's failure identity and cleanup semantics.

The Node/Bun/Deno portability workflow executes these probes on the exact pull-request head and records source-bound receipts. A runtime alias or successful import alone is not sufficient evidence of support.
