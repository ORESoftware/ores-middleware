# Generated peer-authority evidence

This document defines what the independent TypeSpec and JSON Schema/OpenAPI
generation receipts prove. It does not introduce another authority.

## Source and artifact classes

The only editable contract authorities are the human-authored TypeSpec source
and the human-authored JSON Schema/OpenAPI source. Their parsers, normalized
IRs, SQL candidates, language surfaces, ORM models, transport projections, and
receipts are derived evidence.

Common generated artifacts may be compared byte-for-byte only after the output
format has been deliberately canonicalized. A byte match is evidence of the
checked generator versions and inputs, not permission to replace one authored
source with the other. Source-specific artifacts are validated within their own
lane:

- TypeSpec owns the reviewed Protobuf/proto3 and gRPC evidence used by wire
  clients.
- JSON Schema/OpenAPI owns the OpenAPI and HTTP/write-client evidence.
- SQL, common language types/runtime validators, Diesel, and SeaORM are emitted
  independently from both lanes and compared after normalization.

## Canonical generated source

A generator must emit each language's canonical checked-in representation
directly. The acceptance job may run `rustfmt --check`, `gofmt -d`, Gleam
format checking, TypeScript compilation, or another deterministic validator, but
it may not repair one lane after generation and then compare the repaired copy.
A non-empty formatter diff is a generator discrepancy.

Canonical whitespace is part of the generated artifact digest once a language
formatter defines it. Both lanes must independently produce the same canonical
bytes for common artifacts. A formatter version and all relevant options belong
in the execution receipt; changing either invalidates earlier byte-equality
evidence. Source-specific artifacts may differ by design, but each must still be
canonical in its own format and remain traceable to its source authority.

## Required receipt facts

`ores.persistence-polyglot-convergence/v2` must bind, at minimum:

- both source paths and SHA-256 digests;
- the exact generator commit, Rust toolchain, and command/options;
- every emitted artifact path and digest, grouped by source lane;
- direct normalized-model comparison;
- SQL_T versus SQL_J comparison and disposable PostgreSQL catalog read-back;
- generated Rust, TypeScript, Go, Gleam, Elixir, and Erlang compilation and
  shared positive/negative fixtures;
- TypeSpec+Diesel, TypeSpec+SeaORM, JSON-Schema+Diesel, and
  JSON-Schema+SeaORM evidence;
- Protobuf/gRPC and OpenAPI/HTTP projection checks without inventing operations;
- skipped, failed, unavailable, or not-applicable checks as explicit states.

A receipt is `passed` only when the declared scope is complete and every
required check executed successfully. An unexplained difference is
`STOPPED_FOR_EVALUATION`; infrastructure or compiler failure is `failed`; an
incomplete declared scope is `partial`. A previously green receipt cannot be
reused after either source, generator, dependency lock, compiler, database
version, formatter, or comparison option changes.

## Native peer-authority proof

The repair and proof workflow run `33793351801`, job `100775009601`, executed
both independently generated authority lanes against the same bounded positive
and negative corpus in all six native runtime families:

| Authority | Rust | TypeScript | Go | Gleam | Elixir | Erlang |
| --- | --- | --- | --- | --- | --- | --- |
| TypeSpec | executed | executed | executed | executed | executed | executed |
| JSON Schema/OpenAPI | executed | executed | executed | executed | executed | executed |

Before publishing the source repairs, that job proved all of the following:

- eight base peer-authority parity tests passed;
- seven independent persistence-generator tests passed;
- three layered rate-limit contract tests passed;
- both authorities generated matching common SQL, types/interfaces, executable
  validators/code, Diesel witnesses, and SeaORM witnesses;
- the generated-runtime convergence receipt reported `status=passed`,
  `zeroUnexplainedFindings=true`, twelve executed witnesses, and zero
  discrepancies;
- generated Go was `gofmt`-canonical, generated Gleam was formatter-canonical,
  and the Rust, TypeScript, Go, Gleam, Elixir, and Erlang witnesses compiled and
  replayed the shared corpus;
- no authority was selected as a winner and no generated artifact was copied
  back into an authored source directory.

The defects repaired during that proof belonged to their owning implementation
layers rather than to either authority: optional-value handling in the Rust
witness, canonical formatting in the Gleam renderer, argument forwarding in the
Elixir witness, and a legal Erlang type declaration whose runtime validator
still preserves the exact enum vocabulary. The final source commit was
`0c21145edd132f4b2d73aa1548b0801cf71aa18e`. This record is historical evidence,
not permission to reuse that result after the branch head changes; normal
exact-head workflows must execute again before promotion.

## Promotion boundary

No generated artifact is published, copied into an authority directory, used to
plan or apply a migration, or promoted into downstream servers/clients unless
all applicable source, generated-code, ORM, SQL/catalog, and transport checks
are green at the same exact commit. Automation must retain the failing receipt
and stable discrepancy fingerprints before returning failure. It must never
choose the first, fastest, historically preferred, or easiest-to-compile lane as
the winner.

This boundary is enforced by the Rust generator, the generated-polyglot job, the
four-way ORM/PostgreSQL gate, and the repository's existing semantic-contract
and adversarial workflows. Fleet adoption remains tracked in Linear `DEN-3959`,
`DEN-3982`, `DEN-3321`, and `DEN-3828`.
