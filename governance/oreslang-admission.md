# Oreslang integration admission — draft

**Status: BLOCKED / NON-ADMITTED.** This marks Oreslang as a proposed middleware runtime target, not a working runtime and not a third contract authority.

Repository invariant: governance/polyglot-participants.v1.json enforces exact parity of src language dirs, adapter manifests, Zed targets, contractcheck and CI jobs. Do NOT add src/oreslang or a participant before all requirements exist or weaken the allSourceDirectoriesMustBeGoverned rule.

## Required work before enabling a supported target

- Resolve immutable **independent human-authored** TypeSpec and JSON Schema Draft 2020-12 inputs with the owning interface repository. Neither is auto-generated from the other as authored authority.
- Run pinned [TJSV](https://github.com/ORESoftware/typespec-json-schema-validator) on the exact current source closure and valid/invalid/boundary corpus. TJSV owns qualified declarations, Contract IR ID, parity receipt, runtime evidence schema and final admission.
- For persistence-bearing subsets **only**, add deterministic Oreslang witness generation to [ores-contracts](https://github.com/ORESoftware/ores-contracts) using both separately parsed peer IRs; require byte/semantic agreement, negative cases and reproducible output. Do not widen the persistence subset by fabricating ORM state.
- Implement genuine Oreslang syntax, parsing/codec and runtime adapter in [oreslang-serialization-and-validation](https://github.com/ores-truffle-oreslang/oreslang-serialization-and-validation) and the actual consuming package. Compile/run with a pinned real [Java/GraalVM Oreslang compiler](https://github.com/ores-truffle-oreslang/oreslang-source.java); reject skipped or mock-only passes, stale compiler pins and unsupported features.
- Return only bounded and payload-free per-fixture verdicts bound to source and implementation digests, exact Contract IR, parity run ID and corpus hash. Keep field names, enum values, required/optional/null behavior, bounds and rejection behavior identical to admitted contract inputs.
- Add required exact-head cross-language/consumer CI, Zed package manifest, external consumption and negative tests **together** with enabling the target; do not bypass existing governance. If any required lane is missing, stop promotion.
- Oreslang→JS and Oreslang→Wasm/browser remain **future** separate compiler targets requiring real generated JS/Wasm and hermetic browser-import testing, not claims inferred from Rust-Wasm or TypeScript.

### Evidence checklist

- [ ] Complete independent authored peers and reviewed immutable source pins
- [ ] TJSV current-input parity and runtime Contract IR identity
- [ ] Real Oreslang compiler and full positive/negative corpus execution
- [ ] Native adapter/consumer package, Zed and governance CI gates
- [ ] Separately tested JS and Wasm browser backends (future only)

No authored TypeSpec/JSON Schema or generated artifact is changed in this PR. This checklist is a blocker ledger, not a claim of passing CI.
