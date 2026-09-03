# ores-middleware

Cross-language, routing-neutral middleware contracts and adapters for
ORESoftware servers.

The first implemented slice standardizes how middleware recognizes and serves
artifacts produced by [`ORESoftware/api-docs`](https://github.com/ORESoftware/api-docs)
without making the middleware own HTTP routing or API-document generation.
A host adapter supplies a normalized request and receives one of five outcomes:
pass through, serve an artifact, reject the method, reject content negotiation,
or stop because the compiled RPC digest and documentation digest disagree.

## Authority topology

TypeSpec and JSON Schema/OpenAPI are independent, human-authored, top-level
contract authorities:

```text
TypeSpec -> SQL where applicable, Protobuf, gRPC -> wire clients
JSON Schema/OpenAPI -> interfaces/types, SQL where applicable -> write clients
```

Neither lane is an intermediate form of the other. This repository compares the
two authored middleware contracts directly and runs the same behavior fixture
against every implemented language core. SQL/catalog and Diesel/SeaORM parity
remain mandatory upstream admission gates for database-backed contracts; they
are deliberately marked not applicable for this stateless selector rather than
reported as checks that did not run.

## Docs-serving protocol

Recognized aliases:

| Artifact | Paths |
| --- | --- |
| HTML index | `/docs/api`, `/api/docs`, `/api-docs` |
| ORES catalog | `/api/docs.json`, `/api-docs.json` |
| OpenAPI 3.1 | `/openapi.json` |
| OpenRPC | `/openrpc.json` |
| Connect discovery | `/connect.json` |
| JSON Hyper-Schema | `/hyper-schema.json` |

`GET` and `HEAD` are allowed. Other methods on a recognized path produce `405`
with `Allow: GET, HEAD`. Unknown paths produce `pass` so the application router
continues normally.

The three HTML aliases default to HTML but can select another artifact with
`Accept` or `X-Ores-Docs-Format`. Artifact-specific paths must agree with both
headers or return `406`. A configured runtime RPC digest requires a matching,
valid documentation digest; missing or different evidence returns
`stopped-for-evaluation` with `503` and no document body.

The selector returns headers and an artifact key, not bytes. An injected
`api-docs` provider supplies the body. Responses use no-store and browser
hardening headers and expose a verified digest as
`X-Ores-Contract-SHA256`.

## Layout

```text
contracts/          peer TypeSpec and JSON Schema contracts plus audit receipt schema
docs/               architecture and integration boundary
fixtures/            cross-language behavior corpus
src/rust/            Rust core for Tower/Axum/Leptos/Dioxus adapters
src/ts/              TypeScript/JavaScript core for Node, Deno, Bun, Hono, etc.
src/golang/          Go core for net/http, Gorilla, Gin, Echo, and Fiber adapters
src/gleam/           OTP/Gleam adapter boundary
src/elixir/          Plug adapter boundary
src/erlang/          Cowboy adapter boundary
target/<language>/   generated audit/build output; never a contract authority
```

## Verification

The complete CI audit installs pinned compilers and writes an
`ores.schema-audit-receipt/v1` receipt even when a check fails:

```sh
python3 scripts/audit.py --receipt target/audit/receipt.json
python3 scripts/build_targets.py
```

Locally, the dependency-free checks that are available in this environment can
be run independently:

```sh
python3 -m unittest scripts/test_contract_parity.py -v
python3 scripts/check_contract_parity.py
npm test --prefix src/ts
go test ./...
# run from src/golang for the Go command
```

The repository is governed by
[`ORESoftware/my-ai/AGENTS.md`](https://github.com/ORESoftware/my-ai/blob/main/AGENTS.md).
