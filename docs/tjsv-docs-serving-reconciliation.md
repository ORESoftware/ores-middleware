# Docs-serving peer reconciliation (DEN-3959)

This change resolves a specific representation decision, not the four other
blocked TJSV contract lanes, native transport certification, or a release waiver.
It builds on PR #66 and leaves PR #65's adapter/replay work and PR #67's native
persistence evidence intact.

## Reviewed semantics

`DocsRequest` and `DocsDecision` are simple closed object schemas: their own
`properties` list every allowed key, and neither has inheritance, root `$ref`,
conditionals, or same-instance applicators. For those two specific nodes, both
`additionalProperties: false` and `unevaluatedProperties: false` reject exactly
the unlisted keys. Required fields, nullability, bounds, patterns, enums and the
root request/decision union are unchanged. We select the Draft 2020-12
`unevaluatedProperties` spelling explicitly in the authored JSON Schema. This
is not a global normalization of the two keywords; they differ under composition.

`DocsHeaders` is now an explicitly named string dictionary in both authored
lanes, replacing the previously inline dictionary. The TypeSpec `is Record<string>`
model names the same wire shape; the JSON Schema definition permits arbitrary
keys with string values. Neither model is generated from the other. No generated
`RecordString` declaration needs an ignore list or special treatment.

References: JSON Schema Draft 2020-12 core, sections 10.3.2.3 and 11.3
(https://json-schema.org/draft/2020-12/json-schema-core), and the official TypeSpec
JSON Schema emitter reference
(https://typespec.io/docs/emitters/json-schema/reference/emitter/).

## Compatibility and real TJSV evidence

The 87 independent corpus cases cover all five declarations, with positive and
negative expectations for each. They include the six representation values and
five action values, missing/unknown/null/wrong-typed fields, Unicode, own
`__proto__` keys, dictionary value types, exact digest boundaries, and unsigned
16-bit status boundaries. Zero and 65535 remain shape-valid statuses: HTTP status
semantics belong to the runtime, not a silently tightened data-shape contract.

`tests/fixtures/docs-serving-v1.schema.json` is a frozen copy of the original
human-authored schema, verified against original Git blob
`45735e7d6890f31240311636c98be77e5183f69a`. It is compatibility evidence, not a new
contract authority. The AJV 2020 tests require both old and new authored schemas
to agree with every independently chosen expectation; a focused structural check
prohibits any changes outside the reviewed representation changes.

The existing pinned TJSV matrix compiles the changed TypeSpec itself and executes
both independent lanes over synthesized probes plus all 87 corpus cases. The
corpus loader refuses implicit expectations, duplicates, missing positive/negative
coverage, unsafe paths, non-JSON values, stale output directories and incomplete
report consumption. Corpus bytes are SHA-256-bound in the matrix summary; the
source commit and input closure are checked before and after validation. Merely
adding a fixture file without executing it does not pass.

Run the existing `tjsv-contract-boundaries` workflow. It runs the new AJV tests,
corpus tests, prior receipt tests and real compiler controls before all five
contract pairs. The aggregate gate remains red until every remaining contract
pair is reconciled and the other required runtime/review gates pass. No ignore,
waiver, changed validator pin, or continue-on-error approval path is introduced.
