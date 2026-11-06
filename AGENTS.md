# Contributor rules for bzip2-turbo

This is a standalone BZip2 codec extracted from sevenz-turbo. Preserve the
existing stream behavior and compatibility API while introducing standard I/O
adapters. Keep source reference identities and required legal notices intact.

- First pass: API extraction, correctness, ownership and repository hygiene.
  Performance changes are paused. Record the inherited performance baseline;
  any future optimization needs differential byte parity and interleaved median
  measurements. Do not claim a performance gate passed without measurements.
- Malformed input returns an error rather than panicking or reading beyond bounds.
  Preserve reference encoder byte parity and verify against 7-Zip.
- Unsafe blocks require a SAFETY comment and checked ownership invariants.
  Miri verifies portable Rust only; native FFI needs native validation.
- Native sweeps use cargo-nextest 0.9.143 with --no-fail-fast. Run doctests
  separately. Run cargo fmt --all -- --check and cargo clippy --locked
  --workspace --all-targets --all-features -- -D warnings before handoff.
- Do not change dependencies without explicit approval. Before any upgrade read
  renovate.json and honor disabled packages, ranges and version restrictions.
- main is stable. Work on feature/, bugfix/ or hotfix/ branches; worktrees belong
  under .worktrees/. Never reset, clean, restore or stash a shared checkout.
- Commits and operator-created tags must be SSH-signed. The manually dispatched
  release workflow is authorized to create its release tag through GitHub, as
  sevenz-turbo does; that workflow-created tag does not require SSH signing.
  Do not bypass commit signing or push unless explicitly instructed. Release
  and publication require separate authorization.
- Any code change bumps the crate version, `Cargo.lock` and `CHANGELOG.md` in
  the same PR. The bump is what `cargo semver-checks` says it is against the
  published version: the release workflow runs it and refuses a patch bump
  that carries a breaking change. When an open, unreleased version already
  covers the change, add to it instead of bumping again; never add entries
  under a version already on crates.io.
- The changelog section is the released heading, `## <version>`, never one
  marked unreleased. The release workflow rejects an `Unreleased` heading and
  uses that section as the GitHub release notes. Keep the README's dependency
  line showing the version's major.minor.
- Enable hooks with git config core.hooksPath .githooks. Go and gitleaks
  are required. The hooks check complete staged blobs and commit messages.
- Never commit credentials, personal home paths, account names or private
  contact details. Keep machine-specific blocked terms in the external file
  selected by GIT_PRIVACY_BLOCKLIST (GIT_PRIVACY_DENYLIST is also supported).
  Preserve required copyright and license text; do not erase legal attribution.
- Do not commit generated binaries, fixtures, fuzz corpora or benchmark outputs.
  Store raw measurements outside the repository and commit concise summaries.
- Delegated reviewers must explicitly use gpt-6.1-sol.
