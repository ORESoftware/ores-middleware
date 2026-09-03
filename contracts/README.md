# Contract authorities

TypeSpec and JSON Schema/OpenAPI are independent, top-level, human-authored
contract authorities. Neither authority is generated from the other and then
treated as canonical.

The required flows are:

```text
TypeSpec -> SQL_T where applicable, Protobuf, gRPC -> wire clients
JSON Schema/OpenAPI -> interfaces/types/runtime validators, SQL_J where applicable -> write clients
```

In this repository the standard middleware SDK authorities live in
`contracts/typespec/main.tsp` and `contracts/json-schema/`. The routing-neutral
API-document selector has its own directly compared peer pair:

- `contracts/docs-serving.tsp`
- `contracts/docs-serving.schema.json`

The persistence-bearing idempotency model has another independently authored
pair:

- `contracts/persistence/idempotency-record.tsp`
- `contracts/persistence/idempotency-record.schema.json`

The release gate:

1. compares capability and SDK-operation vocabularies between the primary
   TypeSpec and JSON Schema authorities;
2. compares docs-serving enums, properties, requiredness, normalized types, and
   runtime behavior fixtures;
3. projects the idempotency pair independently into SQL_T/SQL_J, client types,
   and Diesel/SeaORM-shaped witnesses;
4. validates fixtures and Draft 2020-12 schemas and compiles each TypeSpec
   source; and
5. compiles and runtime-checks adapter descriptors from every language against
   the standard operation and capability floor.

Generated artifacts are evidence, never replacement authorities. Any
unexplained source, SQL, type, ORM, or runtime discrepancy writes a fingerprint,
blocks publication and adoption, and enters `STOPPED_FOR_EVALUATION`. Never
auto-resolve drift by regenerating one authority from the other or by choosing
one ORM as the winner.
