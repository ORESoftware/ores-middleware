# Runtime convergence for generated artifacts

This document is normative for generated-code admission in this repository. It
generalizes the existing PostgreSQL/Diesel/SeaORM convergence discipline to all
applicable generated structures and executable artifacts.

TypeSpec and JSON Schema/OpenAPI remain independent, human-authored, top-level
authorities. Generated translations, normalized intermediate representations,
source code, protocol descriptors, clients, servers, validators, codecs, ORM
models, migrations, and receipts are evidence. They never become a replacement
authority and never rewrite either authored source.

## Minimum independent-witness matrix

For every generated semantic surface, admission requires both authority lanes to
be consumed by at least two independent implementations whenever two practical
implementations exist:

```text
                         TypeSpec lane       JSON Schema/OpenAPI lane
implementation A         witness A/T         witness A/J
implementation B         witness B/T         witness B/J
```

All four cells must execute at the same exact source commit. The normalized
observations must agree. An unavailable, skipped, inferred, or unexecuted cell is
not success; the receipt is `partial` or `failed` as appropriate.

The fixture corpus is a separate semantic witness. It must be reviewed and
maintained independently rather than generated from either authority.

## Required phases per cell

Each applicable cell records these phases separately:

1. Generate the artifact from its named authority lane.
2. Compile or load it with the named native implementation.
3. Execute shared positive, negative, and boundary fixtures.
4. Normalize accepted values, rejected values, and errors.
5. Serialize and read back accepted values where the artifact is serializable.
6. Compare the cell with the other source lane in the same implementation.
7. Compare it with the other implementation for the same source lane.
8. Compare all cells against the independently maintained fixture expectations.

The receipt must not collapse compilation, execution, normalization, round-trip,
and negative testing into one ambiguous boolean.

## Common structure and validator checks

Generated models, interfaces, validators, decoders, and codecs compare at least:

- exported symbols and visibility;
- source field names, native field names, and wire names;
- declared field order where order is semantically observable;
- logical and native scalar mappings;
- requiredness, optionality, nullability, and defaults;
- enum, union, discriminator, pattern, format, and numeric-bound semantics;
- unknown-field and additional-property behavior;
- normalized accepted values and canonical encoded output;
- stable rejection classes and property paths;
- absence of truncation, widening, silent defaulting, or permissive fallback.

## Protocol and transport artifacts

For Protobuf/gRPC and OpenAPI/HTTP/RPC outputs, source-text compilation is only
the first phase.

The TypeSpec lane must compile Protobuf and read the descriptor set back into a
normalized message/operation graph. Independent generated implementations then
exercise encode/decode and client/server behavior. The JSON Schema/OpenAPI lane
must be parsed through independent OpenAPI readers where practical and exercised
through generated clients and servers.

Normalized comparison includes:

- message and operation names;
- field numbers or wire keys;
- scalar mappings, optionality, enums, unions, and unknown fields;
- operation IDs, methods, paths, parameters, headers, status codes, content
  types, error envelopes, streaming, deadlines, and cancellation;
- requests observed by the conformance server and responses/errors observed by
  each client.

No generator may invent operations when an authored authority declares data
only.

## Configuration, events, caches, and state machines

The same matrix applies to generated configuration loaders, manifests, event
and message envelopes, cache and index structures, and state machines.

Configuration witnesses exercise defaults, precedence, unknown keys, invalid
values, bounds, upgrades, and downgrade rejection. Event witnesses exercise
version compatibility, ordering, duplication, corruption, and replay. Cache
witnesses exercise canonical key bytes, insertion, lookup, eviction, expiry,
restart/reload, corruption, collision boundaries, and bounded resources. State
machine witnesses execute identical valid, invalid, concurrent, retry, timeout,
and recovery traces through every generated implementation.

## ORM and database specialization

The existing persistence gate remains mandatory and stricter:

```text
TypeSpec SQL + Diesel       TypeSpec SQL + SeaORM
JSON Schema SQL + Diesel    JSON Schema SQL + SeaORM
```

The four catalog witnesses compare normalized PostgreSQL structure. They must
also perform data-plane insert/read/update/rejection round trips and compare
normalized rows and failures for nullability, optional fields, integer bounds,
enums/checks, primary keys, unique constraints, timestamps, transactions, and
concurrency behavior.

The generalized runtime gate supplements this database gate; it does not replace
or weaken it.

## Receipt and promotion boundary

Every receipt binds:

- exact commit and source digests for both authorities;
- fixture, generator, dependency-lock, compiler/runtime, database, protocol-tool,
  generated-artifact, and comparison-option digests or versions;
- one explicit record for every declared authority × artifact × implementation
  cell;
- bounded raw logs and normalized outputs;
- deterministic discrepancy fingerprints;
- `passed`, `failed`, `partial`, or `stopped_for_evaluation` without translating
  missing evidence into success.

Any unexplained mismatch enters `STOPPED_FOR_EVALUATION` and blocks publication,
automatic merge, migration, package promotion, client/server adoption, and
deployment. No historically preferred authority, generator, runtime, ORM, parser,
or previously green commit wins by fallback. A prior receipt is invalid after a
change to either source, fixture, generator, lockfile, runtime/compiler, database,
protocol tool, or comparison option.

## Tracking

- GitHub epic: `ORESoftware/ores-middleware#30`
- Active implementation: `ORESoftware/ores-middleware#28`
- Rust migration of remaining control tooling: `ORESoftware/ores-middleware#18`
- SQL/ORM/catalog precedent: `ORESoftware/ores-middleware#5` and PR `#11`
- Linear: `DEN-3321`, `DEN-3959`, `DEN-3982`, `DEN-4078`, `DEN-3176`, and
  `DEN-3828`

The repository audit and zed-pkg admission path must keep this policy executable;
documentation alone is not release evidence.
