# Zed release provenance

The middleware Zed release has two separate admission gates. They are intentionally not collapsed into one network-dependent test.

## 1. Credential-free reproducibility

`zed-release-acceptance.yml` and `scripts/run-zed-release-acceptance.sh` validate the authored package without registry credentials. They validate package metadata, run every native target test, pack the complete seven-artifact set twice, compare exact archive names and SHA-256 digests, and run the typed archive auditor against the canonical repository artifact.

This gate proves that the reviewed source deterministically produces the reviewed package bytes. It does **not** claim that an isolated throwaway file registry contains external package dependencies.

## 2. Registry dependency closure

A release branch resolves every direct dependency into `.zpkg.lock`. Each direct lock entry must carry an exact resolved version, artifact SHA-256, positive byte size, archive format, source locator, VCS tag, and 40-character VCS commit. The resolver may mutate only `.zpkg.lock`.

After merge, `release-zed.yml` is the single owner of irreversible release actions. It verifies the exact main commit, refuses to rewrite an existing immutable `v{version}` tag, publishes through the protected `zed-pkg` environment, and installs the exact published version into a clean consumer before running the installed-package smoke test and a frozen reinstall.

## Ownership rules

- `.zpkg.toml` is the authored package/version declaration.
- `.zpkg.lock` is resolver-produced provenance evidence, never hand-authored release metadata.
- `zed-source-tag.yml` may validate policy and resolve a release lock, but it must not create or rewrite release tags.
- `release-zed.yml` alone owns immutable tag creation and registry publication.
- Release helpers derive the middleware version from `.zpkg.toml`; they must not hard-code the current package version.
- The existing `v0.1.0` tag is immutable even when later source is superior. New reviewed source receives a new version/tag.

A red check that executed source or package logic is a release blocker. A GitHub Actions job that never acquired a runner and has no executed steps is infrastructure evidence only; it must not be represented as a passed release test.
