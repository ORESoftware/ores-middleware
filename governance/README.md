# Polyglot governance

The participant registry is the machine-readable authority for the six maintained
runtime implementations. The generic conformance manifest is currently bootstrap
metadata only; behavioral admission is delegated to the exact-head
`contract-conformance.yml` language jobs.

That delegation is itself governed. If the manifest remains `scaffold-only`, the
registry and manifest must identify the same workflow, the delegation must be
fail-closed, and every governed language must still execute its runtime tests,
contract checker, and runtime-descriptor check in that workflow.
