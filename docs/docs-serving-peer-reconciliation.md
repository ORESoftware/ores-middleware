# Docs-serving peer reconciliation — DEN-3959

## Semantic integration of PR #69 with landed #70

PR #69 originally reconciled the docs-serving sources by naming a public
`DocsHeaders` map and recording both additional- and unevaluated-property
constraints. All nine workflows passed at `9b0b453ba7010d143f392e36b2117701d722988e`.
Before it merged, main landed #70 at `9063e28e32c147b923a7dbf85712af169f8d9d3f`:
four public declarations, an anonymous inline string map, explicit model closure,
a newer pinned TJSV CLI gate, and narrowly scoped Rust preflight compatibility.

The integration preserves intent instead of combining incompatible representations:

- Keep #70's independently authored TypeSpec and JSON Schema sources byte-for-byte.
  A fifth public map identity is unnecessary now that the anonymous expression is
  compiler-tested, and would break the landed four-declaration inventory.
- Keep the 133 independently expected cases from #69 byte-for-byte. `DocsHeaders`
  in that fixture module is now a **test subject label**, not a public declaration.
  Its 15 cases test the actual inline subschema in both historical and current
  schemas. Every one must also have an identical enclosing `DocsDecision` case.
- Execute the other 118 full declaration cases through real TJSV. No header case
  is ignored: header-only checks and their enclosing wire witnesses are distinct
  checks of the same retained expectations.
- Reuse the landed TJSV revision and receipt-admission policy from
  `scripts/check-tjsv-docs.mjs`, rather than maintaining a second tool pin or
  weakening source constraints. Emitter options follow that explicit flat-model
  policy. Neither source is regenerated or overwritten from its peer.
- Retain all of main's native evidence/filesystem, Rust transport, ORM, release,
  configuration and ignore changes. The only changes relative to that main are
  this document and the additional compatibility/rejection test files/workflow.

Both histories are retained by an ordinary two-parent merge. The original named
representation remains in Git ancestry for provenance; it is not silently claimed
as a current public type. No rebase, reset, stash, force-push or branch deletion is
needed. Earlier receipts describe their original heads, never the merged head.

## Executable compatibility and rejection proof

The baseline comes from the real historical schema at
`6183cc877d6c058349adc733d325297c07d1c063`, not a reconstructed approximation.
All 133 cases must match their independent expectations against both that schema
and the current authored schema. The root union and declaration inventory are
also asserted. Fixture coverage includes optionality, unknown fields, nulls,
wrong types, enum values, digests, uint16 boundaries, empty maps, Unicode values,
and literal `__proto__` / `constructor` keys.

Actual pinned `ORESoftware/typespec-json-schema-validator` compiles TypeSpec and
compares all four declarations with all 118 wire cases plus synthesized probes.
Missing/extra declarations, skipped coverage, compiler failure, findings,
refusals, corpus loss, or source/tool mutation fail the gate.

Seven real rejection controls run after that positive baseline:

1. Change only the authored method scalar from string to integer.
2. Open the authored request object to unknown properties.
3. Open the authored decision object to unknown properties.
4. Widen authored uint16 status to admit 65536.
5. Change authored header values from strings to integers.
6. Remove an authored digest pattern.
7. Keep both peers unchanged but contradict an independent corpus expectation,
   including a value already present in the invalid lane.

The first six must produce structural findings AND observed verdict divergence,
with no refused validators and complete corpus accounting. The seventh must stop
for a differential/corpus finding with zero structural findings. A failed child
test cannot produce a successful final receipt. All reports are retained in a
fresh per-run directory, and `receipt.json` is written only after every required
check and final source/tool integrity verification succeeds.

## Running and scope

`.github/workflows/docs-serving-peer-contract.yml` checks the exact source head,
reads the immutable TJSV policy already used by the landed docs gate, installs
locked dependencies, and builds only its reviewed flags-2-env native binding.
Permissions remain read-only and checkout credentials are not persisted.

With the pinned tool installed at `tmp/tjsv`, run:

```sh
node --test tests/docs-serving-peers.test.mjs
```

Evidence is written below `target/docs-serving-peers/`. These finite schema and
compatibility tests do not certify universal equivalence, transport behavior or
all frameworks. Existing TJSV CLI, native runtime, Rust socket, generated-language,
ORM/persistence, release and review gates remain necessary at the current head.
The broader #65/#66 branches need their own semantic integration and fresh tests.
