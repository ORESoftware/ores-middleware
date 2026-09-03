# Agent instructions for `ORESoftware/ores-middleware`

This repository inherits and must follow
[`ORESoftware/my-ai/AGENTS.md`](https://github.com/ORESoftware/my-ai/blob/main/AGENTS.md)
and `original-agents.md`.

## Contract authority

TypeSpec and JSON Schema/OpenAPI are independent, human-authored, top-level
contract authorities. Neither may be generated from, subordinated to, used as a
fallback for, or allowed to overwrite the other.

Required peer flows:

```text
TypeSpec -> SQL where applicable, Protobuf, gRPC -> wire clients
JSON Schema/OpenAPI -> interfaces/types, SQL where applicable -> write clients
```

Generated translations are comparison witnesses only. Any unexplained
TypeSpec/JSON Schema, SQL/catalog, generated-type/client, Diesel/SeaORM, or
runtime-conformance mismatch enters `STOPPED_FOR_EVALUATION`. Automation must
publish a discrepancy receipt and must not choose a winner by source order,
timing, historical rank, or generator preference.

## Repository boundary

The core middleware contract is routing-neutral. It may inspect a normalized
method, path, `Accept`, and `X-Ores-Docs-Format`, then return a decision. It must
not own an application router, open a listener, terminate TLS, generate API
documents, decode RPC payloads, or call a persistence provider.

`ORESoftware/api-docs` owns the RPC route-map catalog and digest-bound OpenAPI,
OpenRPC, Connect, Hyper-Schema, and language surfaces. Framework adapters in
this repository translate host requests into the neutral decision contract and
ask an injected provider for the selected artifact.

Keep framework-specific code outside the language core. Unknown paths must pass
through so the host router retains ownership. Do not log or reflect
`Authorization`, cookies, API keys, or other credentials.

## Delivery

Work on a feature branch, stage explicit paths, run the repository audit, push,
and open a pull request. Do not rebase, stash, reset, force-push, or commit
credentials. A failed parity gate must leave an inspectable receipt under
`target/` and the PR must remain draft or blocked until human evaluation.
