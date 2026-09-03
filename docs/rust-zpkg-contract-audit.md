# Typed Rust Zed repository-contract authority

`tools/contract-parity/src/bin/zpkg_contract_check.rs` is the authoritative
static semantic checker for this repository's `.zpkg.toml` contract. It
validates package identity, the six language target slices, build outputs,
installed-package smoke closure, workspace command bindings, required gate
files, and the independent TypeSpec/JSON Schema authority topology.

The checker emits `ores.zpkg-contract-audit/v1` with stable typed finding codes,
source SHA-256 digests, `passed` or `stopped_for_evaluation`, and no timestamps
or host-specific paths. Identical inputs therefore produce byte-identical
receipts.

`scripts/check_zpkg.py` remains only as a compatibility entry point for existing
npm, Justfile, audit, and Zed hooks. It invokes the Rust binary and reads its
receipt; it contains no second semantic implementation. CI compares the direct
Rust receipt with the Python-shim receipt byte-for-byte.

This static authority does not replace the canonical Zed parser. The separate
`zed-release-acceptance` workflow still installs checksum-pinned Zed 0.2.3,
packs twice, verifies byte reproducibility, inspects the archive, installs it in
an isolated consumer, and executes the installed six-language closure. Both the
Rust semantic receipt and the real Zed pack/install receipt must pass.

TypeSpec and JSON Schema/OpenAPI remain independent, human-authored, top-level
authorities. The Rust checker rejects removal of either peer, authority
precedence in either direction, missing translation/round-trip/catalog/ORM
gates, or any mismatch policy other than `STOPPED_FOR_EVALUATION`.
