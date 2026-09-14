# TJSV cross-language drift barrier

`ores-middleware` treats cross-language agreement as executable evidence, not as a claim that six hand-written SDKs happen to look similar.

The contract stack has two independently authored engineering authorities:

1. TypeSpec source; and
2. JSON Schema Draft 2020-12 source.

`ORESoftware/typespec-json-schema-validator` (TJSV) is the fail-closed parity and evidence gate between those peers. TypeSpec-generated JSON Schema, Contract IR, generated clients, runtime validators, language artifacts, Protobuf/WIT/OpenAPI projections, and receipts are evidence only. None of them may silently overwrite or outrank either authored authority.

## Four enforcement layers

### 1. Compile-time / source parity

Every contract family that crosses a language/runtime boundary must run the real pinned TJSV compiler-backed check before promotion:

- inventory the complete TypeSpec declaration set;
- emit the TypeSpec JSON Schema comparison witness;
- validate both JSON Schema lanes as Draft 2020-12;
- compare declaration identity and normalized structure;
- run differential probes and the independent positive/negative corpus in both directions;
- require zero unexplained findings and zero differential refusals;
- retain the deterministic parity `runId` and verified Contract IR.

A generated schema is never copied over the authored JSON Schema to make a red gate green. Structural disagreement remains visible even when a finite probe corpus happens to agree.

### 2. Build-time artifact admission

A language build is admissible only when it can be tied to the exact parity-approved contract inputs.

For Rust, TypeScript/JavaScript, Go, Gleam, Elixir, and Erlang, build/promotion evidence should use TJSV's public language-boundary contract and bind at least:

- immutable source revision;
- generated artifact SHA-256 digest;
- exact parity receipt `runId`;
- exact Contract IR ID;
- language + runtime identity;
- generator + toolchain identity/version;
- explicit ingress and egress validation results.

The promotion orchestrator should use `verifyLanguageBoundariesAgainstCurrentInputs()` (or an equivalent trusted wrapper) against the current checkout. Evidence from an earlier source revision, an earlier parity run, or another Contract IR must stop promotion.

`npm run contracts:admitted-build` is the explicit local/CI entrypoint when the pinned TJSV checkout has already been provisioned. The repository's TJSV workflows remain responsible for provisioning the immutable tool revision and retaining failure evidence.

### 3. Native/runtime conformance before release

Generated or hand-written validators in different runtimes must execute the same trusted corpus and emit TJSV runtime-conformance evidence. Adapter agreement is corroborating evidence; it does not create a third contract authority.

The repository already uses this pattern for native persistence evidence across Node.js, Rust, Go, Gleam, Elixir, and Erlang. Expand that matrix contract-family-by-contract-family rather than maintaining unrelated per-language fixture expectations.

Required rule: the expected verdict belongs to the trusted corpus/reference admission layer. A runtime adapter must never grade itself by writing both `expected` and `actual`.

### 4. Cheap runtime drift observation

Runtime checking supplements compile/build gates; it does not replace them.

There are two intentionally cheap checks:

1. **Artifact binding at startup/registration** — O(1) comparisons of the loaded artifact's TJSV evidence against the expected `runId`, Contract IR ID, source revision, language, and runtime. This does not invoke the TypeSpec compiler.
2. **Optional request/message shadow verdict** — the normal runtime validator decides request admission while a parity-approved reference validator returns only `accepted`, `rejected`, or `refused`. A drift event is emitted only if the verdicts disagree or the reference lane refuses evaluation.

A normal invalid request is **not** drift when both validators reject it. Logging every client validation failure as language drift would create false alarms and hide real contract divergence.

The TypeScript middleware exposes these runtime primitives through `@oresoftware/ores-middleware/contract-drift`. `RequestContractMatch.referenceValidate` is the shadow/reference lane; `RequestContractValidator.driftObserver` receives bounded drift findings. `createOresOtelMiddleware` automatically adds an `ores-otel` observer when the validator does not already provide one.

## ores-otel event contract

Runtime drift is logged as the stable event:

```text
event.name = ores.contract.drift
message    = contract runtime drift
```

Allowed bounded fields are:

- `contract.drift_kind`;
- `contract.operation_id`;
- `contract.declaration`;
- `contract.language`;
- `contract.runtime`;
- `contract.runtime_verdict`;
- `contract.reference_verdict`.

Do **not** put raw request/response payloads, arbitrary headers, schema fragments, credentials, user/tenant identifiers, stack traces, parity digests, or Contract IR digests into the drift event. Full digests and provenance belong in retained TJSV receipts/artifacts, not high-cardinality runtime metrics/log dimensions.

Telemetry delivery is detached from request outcome. A logging/exporter failure must not turn a valid request into a failure or change the response selected by the primary validator.

## Drift categories

The portable runtime vocabulary is intentionally small:

- `evidence_malformed`;
- `evidence_not_passed`;
- `ingress_validation_failed`;
- `egress_validation_failed`;
- `receipt_run_id_mismatch`;
- `contract_ir_id_mismatch`;
- `source_revision_mismatch`;
- `language_mismatch`;
- `runtime_mismatch`;
- `runtime_verdict_divergence`;
- `reference_validation_refused`.

These are operational categories, not a replacement for TJSV's richer build-time finding taxonomy.

## Language implementation rule

Do not independently redesign this protocol in each language. The portable JSON evidence contracts and TJSV receipt identities are the interop boundary. Each language may project them into idiomatic types, but it must preserve:

- the same evidence schema/version;
- the same verdict vocabulary;
- the same drift-category vocabulary;
- fail-closed treatment of malformed/stale evidence;
- no raw payloads in drift telemetry;
- the rule that adapter/runtime evidence is non-authoritative.

Rust may use strongly typed enums/structs, TypeScript discriminated unions, Go structs/interfaces, and BEAM records/maps; those are language projections of one wire/evidence contract, not six new contracts.

## Formal behavior and operation signatures

JSON Schema covers data shapes, not complete TypeSpec operation behavior. For operation/function coherence:

- use TJSV TypeSpec behavioral metadata (`@behavior` / `@behaviorRef`) for stable operation identities and behavioral contracts;
- project operation IDs into OpenAPI/Protobuf/WIT without copying competing behavior authorities;
- use Dafny/CEL/CUE/Rego or another reviewed formal/executable behavior authority for invariants that cannot be represented as data schemas;
- compile every consumer implementation against generated/verified function signatures;
- retain behavioral conformance evidence separately from data-shape parity evidence.

This keeps TypeSpec/JSON Schema parity, generated function signatures, formal behavior, and runtime conformance connected without pretending JSON Schema alone proves function bodies.

## Promotion rule

A release or Zed package promotion should be blocked when any required layer is missing:

```text
peer TypeSpec + JSON Schema
          |
          v
     TJSV parity PASS
          |
          +--> verified Contract IR
          |
          v
   language artifact build
          |
          v
language-boundary evidence PASS
          |
          v
native runtime-conformance PASS
          |
          v
        promote
```

Runtime drift observation then provides a production canary for mistakes that escaped bounded pre-release evidence. A runtime drift event should create an investigation signal; it is never used to retroactively declare a stale build receipt valid.
