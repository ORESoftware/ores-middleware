# Initial TJSV findings: DEN-3959

This is an audit result, not a parity certification or permission to bypass the
new gate. The first hosted run executed the actual pinned validator successfully
and reported `stopped_for_evaluation` for all five source pairs.

- Source: `fe1d39effc77e39606a807994ea2fb1ef84be0c5`
- TJSV: `4473504c4c9d2831d825919f70c03994d8ce01d2`
- Workflow run: https://github.com/ORESoftware/ores-middleware/actions/runs/34295952295
- Artifact ID: `10083133930`
- Artifact SHA-256: `8b0a3ac8bf6ccd66ca28750fde324415d76a41360358d60f4d94470a20a874ab`

| Surface | Structural finding records | Compared declarations | Probes executed |
| --- | ---: | ---: | ---: |
| middleware | 89 | 0 | 0 |
| docs-serving | 8 | 4 | 213 |
| function-bodies | 30 | 0 | 0 |
| persistence | 3 | 2 | 132 |
| rate-limit-v2 | 20 | 0 | 0 |

The 150 records are not 150 confirmed behavioral bugs. Most are declaration
identity or representation discrepancies. Zero differential findings in an
unmatched lane means **no paired declarations were exercised**, not that the
runtimes or authorities agree. The 345 probes in the two paired lanes found no
verdict divergences or validation refusals; this finite result does not prove
universal equivalence and does not override structural findings.

## Required semantic reconciliation

Middleware and function-body schemas use declaration identities different from
the TypeSpec names; middleware also reports a duplicate schema declaration.
Rate-limit v2 has six authored declarations versus seven TypeSpec/generated
ones. Inspect root-model discovery and the complete declaration inventories
before proposing reviewed one-to-one mappings. Do not hide extra or missing
models with ignore lists or count an unmatched declaration as tested.

Docs serving uses inline header maps while the emitter introduces `RecordString`
as a separate declaration/reference. Its closed object schemas also use
`additionalProperties: false`, while the emitter uses
`unevaluatedProperties: false`. Persistence reports the same closure-spelling
difference and an authored-only `x-ores-sql` table/primary-key/unique annotation.
These require semantic review together with the existing runtime and ORM
witnesses; no production authority was mechanically rewritten in this slice.

## Control-fixture correction

The first positive control used `additionalProperties: false` in its single,
non-composed authored model. That accepts the same relevant values as the
emitter's `unevaluatedProperties: false`, but TJSV intentionally reports their
structural difference. The corrected synthetic control spells its intended
closure with `unevaluatedProperties`, without changing production contracts or
weakening comparison settings. The contradictory-corpus control additionally
requires zero structural findings so its failure must come from the independent
instance expectation, not incidental source differences.

All actual source-pair discrepancies remain blocking. A later execution must
produce receipts for its own exact head; this historical run cannot certify a
new revision.
