# Contract authorities

`contracts/typespec/main.tsp` and the schemas in `contracts/json-schema/` are independent, top-level, human-authored authorities.

Neither authority is generated from the other. TypeSpec emits SQL/OpenAPI/Protobuf/gRPC-facing artifacts as configured by downstream repositories; JSON Schema/OpenAPI independently drives runtime validation, interface/type generation, SQL projections, and clients. Generated artifacts are evidence, never a replacement authority.

The release gate performs three comparisons:

1. Direct parity of capability and SDK-operation vocabularies between TypeSpec and JSON Schema.
2. Validation of canonical fixtures against the JSON Schema authority and TypeSpec compilation.
3. Runtime adapter descriptors from every language against `adapter-descriptor.schema.json`, followed by equality checks for the required operation set and capability floor.

Any discrepancy fails closed and requires a human evaluation. Do not auto-resolve a mismatch by regenerating one authority from the other.
